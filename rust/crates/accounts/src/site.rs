//! Website-facing routes: live server list, filtered leaderboard, player stats,
//! teams, support tickets, creator requests, stream overlay, competitions, and
//! the small admin API (`X-Admin-Key`) that answers tickets and bans accounts.

use axum::{
    Json, Router,
    extract::{FromRequestParts, Path, Query, State},
    http::{StatusCode, header, request::Parts},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use rusqlite::{Connection, params};
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::{
    ApiError, AppState, DbErr,
    crypto::{random_token, sha256_hex},
    routes::{Auth, ClientIp, ServerAuth, rate},
};

pub const REGIONS: [&str; 5] = ["EU", "NA", "SEA", "OCE", "BR"];
const OVERLAY_FIELDS: [&str; 9] = ["rank", "score", "kills", "wins", "kd", "winrate", "games", "deaths", "best"];
const SERVER_FRESH_SECONDS: i64 = 120;

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS match_players (
    server     TEXT NOT NULL,
    match_id   TEXT NOT NULL,
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    kills      INTEGER NOT NULL,
    deaths     INTEGER NOT NULL,
    score      INTEGER NOT NULL,
    won        INTEGER NOT NULL,
    mode       TEXT NOT NULL,
    region     TEXT NOT NULL,
    duration   INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (server, match_id, account_id)
);
CREATE INDEX IF NOT EXISTS match_players_account ON match_players(account_id);
CREATE TABLE IF NOT EXISTS live_servers (
    name        TEXT PRIMARY KEY,
    region      TEXT NOT NULL,
    mode        TEXT NOT NULL,
    map         TEXT NOT NULL,
    players     INTEGER NOT NULL,
    max_players INTEGER NOT NULL,
    seen_at     INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS teams (
    id         INTEGER PRIMARY KEY,
    name       TEXT NOT NULL UNIQUE COLLATE NOCASE,
    tag        TEXT NOT NULL UNIQUE COLLATE NOCASE,
    created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS team_members (
    team_id    INTEGER NOT NULL REFERENCES teams(id) ON DELETE CASCADE,
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    role       TEXT NOT NULL,
    PRIMARY KEY (team_id, account_id)
);
CREATE TABLE IF NOT EXISTS support_tickets (
    id         INTEGER PRIMARY KEY,
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    category   TEXT NOT NULL,
    subject    TEXT NOT NULL,
    body       TEXT NOT NULL,
    status     TEXT NOT NULL DEFAULT 'open',
    reply      TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS creator_requests (
    id         INTEGER PRIMARY KEY,
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    twitch     TEXT NOT NULL,
    youtube    TEXT NOT NULL,
    x          TEXT NOT NULL,
    kind       TEXT NOT NULL,
    title      TEXT NOT NULL,
    message    TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS overlays (
    account_id INTEGER PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    token      TEXT NOT NULL UNIQUE,
    fields     TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS competitions (
    id         INTEGER PRIMARY KEY,
    kind       TEXT NOT NULL,
    title      TEXT NOT NULL,
    creator    TEXT NOT NULL,
    mode       TEXT NOT NULL,
    region     TEXT NOT NULL,
    status     TEXT NOT NULL,
    created_at INTEGER NOT NULL
);
";

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/servers", get(servers))
        .route("/servers/heartbeat", post(heartbeat))
        .route("/me/stats", get(my_stats))
        .route("/teams", get(teams_list).post(team_create))
        .route("/teams/{id}", delete(team_delete))
        .route("/teams/{id}/leave", post(team_leave))
        .route("/tickets", get(tickets_list).post(ticket_create))
        .route("/creator-requests", post(creator_request))
        .route("/overlay", get(overlay_get).put(overlay_put))
        .route("/overlay/regenerate", post(overlay_regen))
        .route("/overlay/{token}", get(overlay_public))
        .route("/competitions", get(competitions))
        .route("/admin/tickets", get(admin_tickets))
        .route("/admin/tickets/{id}", post(admin_ticket_reply))
        .route("/admin/creator-requests", get(admin_creator_requests))
        .route("/admin/competitions", post(admin_competition))
        .route("/admin/accounts/{id}/ban", post(admin_ban))
}

// ---- helpers --------------------------------------------------------------------------

pub struct AdminAuth;

impl FromRequestParts<AppState> for AdminAuth {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let key = parts.headers.get("x-admin-key").and_then(|v| v.to_str().ok()).ok_or_else(ApiError::unauthorized)?;
        match &state.cfg.admin_key {
            Some(h) if *h == sha256_hex(key.trim()) => Ok(AdminAuth),
            _ => Err(ApiError::unauthorized()),
        }
    }
}

/// Text with no control characters (newlines allowed when `multiline`), within `max` characters.
fn text(t: &str, min: usize, max: usize, multiline: bool) -> bool {
    let n = t.chars().count();
    n >= min && n <= max && t.chars().all(|c| !c.is_control() || (multiline && (c == '\n' || c == '\r')))
}

/// SQL LIKE pattern for a "contains" search, escaping the wildcard characters.
fn like(q: &str) -> String {
    let mut s = String::from("%");
    for c in q.chars() {
        if matches!(c, '%' | '_' | '\\') {
            s.push('\\');
        }
        s.push(c);
    }
    s.push('%');
    s
}

// ---- live servers --------------------------------------------------------------------

async fn servers(State(s): State<AppState>) -> Result<Response, ApiError> {
    let cutoff = s.now() - SERVER_FRESH_SECONDS;
    let rows = s
        .db(move |c| {
            let mut st = c.prepare("SELECT name, region, mode, map, players, max_players FROM live_servers WHERE seen_at > ?1 ORDER BY region, name")?;
            let v = st
                .query_map([cutoff], |r| {
                    Ok(json!({ "name": r.get::<_, String>(0)?, "region": r.get::<_, String>(1)?, "mode": r.get::<_, String>(2)?,
                               "map": r.get::<_, String>(3)?, "players": r.get::<_, i64>(4)?, "max_players": r.get::<_, i64>(5)? }))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(v)
        })
        .await?;
    Ok(([(header::CACHE_CONTROL, "public, max-age=15")], Json(json!({ "servers": rows }))).into_response())
}

#[derive(Deserialize)]
struct Heartbeat {
    region: String,
    mode: String,
    map: String,
    players: i64,
    max_players: i64,
}

/// A game server says it is up. Servers that stop reporting vanish from the list.
async fn heartbeat(State(s): State<AppState>, server: ServerAuth, Json(b): Json<Heartbeat>) -> Result<StatusCode, ApiError> {
    if !REGIONS.contains(&b.region.as_str()) || !text(&b.mode, 1, 20, false) || !text(&b.map, 1, 40, false) || !(1..=256).contains(&b.max_players) || !(0..=b.max_players).contains(&b.players) {
        return Err(ApiError::bad("invalid_input"));
    }
    let now = s.now();
    let name = server.0;
    s.db(move |c| {
        c.execute(
            "INSERT INTO live_servers (name, region, mode, map, players, max_players, seen_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
             ON CONFLICT(name) DO UPDATE SET region = ?2, mode = ?3, map = ?4, players = ?5, max_players = ?6, seen_at = ?7",
            params![name, b.region, b.mode, b.map, b.players, b.max_players, now],
        )?;
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---- leaderboard and stats -------------------------------------------------------------

pub struct Board {
    pub region: Option<String>,
    pub mode: Option<String>,
    pub since: i64,
    pub q: Option<String>,
    pub account: Option<i64>,
    pub limit: i64,
}

/// Ranked players. A player's score is the sum of their ten best games in the filter.
pub fn board(c: &Connection, f: &Board) -> Result<Vec<Value>, DbErr> {
    let mut st = c.prepare(
        "SELECT rank, callsign, top, games, wins, kills, deaths, best FROM ( \
           SELECT ROW_NUMBER() OVER (ORDER BY top DESC, id ASC) AS rank, id, callsign, top, games, wins, kills, deaths, best FROM ( \
             SELECT a.id AS id, a.callsign AS callsign, \
                    SUM(CASE WHEN r.rn <= 10 THEN r.score ELSE 0 END) AS top, COUNT(*) AS games, SUM(r.won) AS wins, \
                    SUM(r.kills) AS kills, SUM(r.deaths) AS deaths, MAX(r.score) AS best \
             FROM (SELECT account_id, score, kills, deaths, won, \
                          ROW_NUMBER() OVER (PARTITION BY account_id ORDER BY score DESC) AS rn \
                   FROM match_players \
                   WHERE (:region IS NULL OR region = :region) AND (:mode IS NULL OR mode = :mode) AND created_at >= :since) r \
             JOIN accounts a ON a.id = r.account_id WHERE a.banned = 0 GROUP BY a.id)) \
         WHERE (:q IS NULL OR callsign LIKE :q ESCAPE '\\') AND (:acct IS NULL OR id = :acct) \
         ORDER BY rank LIMIT :limit",
    )?;
    let rows = st
        .query_map(
            rusqlite::named_params! { ":region": f.region, ":mode": f.mode, ":since": f.since, ":q": f.q.as_deref().map(like), ":acct": f.account, ":limit": f.limit },
            |r| {
                Ok(json!({ "rank": r.get::<_, i64>(0)?, "callsign": r.get::<_, String>(1)?, "score": r.get::<_, i64>(2)?,
                           "games": r.get::<_, i64>(3)?, "matches": r.get::<_, i64>(3)?, "wins": r.get::<_, i64>(4)?,
                           "kills": r.get::<_, i64>(5)?, "deaths": r.get::<_, i64>(6)?, "best": r.get::<_, i64>(7)? }))
            },
        )?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[derive(Deserialize)]
pub struct LeaderQuery {
    limit: Option<i64>,
    region: Option<String>,
    mode: Option<String>,
    season: Option<String>,
    q: Option<String>,
}

pub async fn leaderboard(State(s): State<AppState>, Query(q): Query<LeaderQuery>) -> Result<Response, ApiError> {
    let region = q.region.filter(|r| !r.is_empty());
    if region.as_deref().is_some_and(|r| !REGIONS.contains(&r)) {
        return Err(ApiError::bad("invalid_input"));
    }
    let mode = q.mode.filter(|m| !m.is_empty());
    if mode.as_deref().is_some_and(|m| !text(m, 1, 20, false)) {
        return Err(ApiError::bad("invalid_input"));
    }
    let search = q.q.map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
    if search.as_deref().is_some_and(|t| !text(t, 1, 32, false)) {
        return Err(ApiError::bad("invalid_input"));
    }
    let day = 86_400;
    let since = match q.season.as_deref().unwrap_or("all") {
        "" | "all" | "current" => 0,
        "30d" => s.now() - 30 * day,
        "7d" => s.now() - 7 * day,
        _ => return Err(ApiError::bad("invalid_input")),
    };
    let f = Board { region, mode, since, q: search, account: None, limit: q.limit.unwrap_or(25).clamp(1, 100) };
    let rows = s.db(move |c| board(c, &f)).await?;
    Ok(([(header::CACHE_CONTROL, "public, max-age=30")], Json(rows)).into_response())
}

async fn my_stats(State(s): State<AppState>, auth: Auth) -> Result<Json<Value>, ApiError> {
    let id = auth.account.id;
    let v = s
        .db(move |c| {
            Ok(c.query_row(
                "SELECT COALESCE(SUM(duration), 0), COUNT(*), COALESCE(SUM(kills), 0), COALESCE(SUM(deaths), 0), COALESCE(SUM(won), 0) \
                 FROM match_players WHERE account_id = ?1",
                [id],
                |r| Ok(json!({ "playtime_secs": r.get::<_, i64>(0)?, "matches": r.get::<_, i64>(1)?, "kills": r.get::<_, i64>(2)?,
                               "deaths": r.get::<_, i64>(3)?, "wins": r.get::<_, i64>(4)? })),
            )?)
        })
        .await?;
    Ok(Json(v))
}

// ---- teams -------------------------------------------------------------------------------

async fn teams_list(State(s): State<AppState>, auth: Auth) -> Result<Json<Value>, ApiError> {
    auth.web()?;
    let id = auth.account.id;
    let teams = s
        .db(move |c| {
            let mut st = c.prepare(
                "SELECT t.id, t.name, t.tag, m.role, (SELECT COUNT(*) FROM team_members x WHERE x.team_id = t.id) \
                 FROM teams t JOIN team_members m ON m.team_id = t.id WHERE m.account_id = ?1 ORDER BY t.id",
            )?;
            Ok(st
                .query_map([id], |r| Ok(json!({ "id": r.get::<_, i64>(0)?, "name": r.get::<_, String>(1)?, "tag": r.get::<_, String>(2)?, "role": r.get::<_, String>(3)?, "members": r.get::<_, i64>(4)? })))?
                .collect::<Result<Vec<_>, _>>()?)
        })
        .await?;
    Ok(Json(json!({ "teams": teams })))
}

#[derive(Deserialize)]
struct TeamBody {
    name: String,
    tag: String,
}

fn make_team(c: &mut Connection, acct: i64, name: &str, tag: &str, now: i64) -> Result<Result<i64, &'static str>, DbErr> {
    let tx = c.transaction()?;
    let count = |sql: &str, p: &str| -> Result<i64, DbErr> { Ok(tx.query_row(sql, [p], |r| r.get(0))?) };
    let acct_s = acct.to_string();
    if count("SELECT COUNT(*) FROM team_members WHERE account_id = ?1", &acct_s)? >= 10
        || count("SELECT COUNT(*) FROM team_members WHERE account_id = ?1 AND role = 'leader'", &acct_s)? >= 4
    {
        return Ok(Err("too_many_teams"));
    }
    if count("SELECT COUNT(*) FROM teams WHERE name = ?1", name)? > 0 {
        return Ok(Err("name_taken"));
    }
    if count("SELECT COUNT(*) FROM teams WHERE tag = ?1", tag)? > 0 {
        return Ok(Err("tag_taken"));
    }
    tx.execute("INSERT INTO teams (name, tag, created_at) VALUES (?1, ?2, ?3)", params![name, tag, now])?;
    let id = tx.last_insert_rowid();
    tx.execute("INSERT INTO team_members (team_id, account_id, role) VALUES (?1, ?2, 'leader')", params![id, acct])?;
    tx.commit()?;
    Ok(Ok(id))
}

async fn team_create(State(s): State<AppState>, auth: Auth, Json(b): Json<TeamBody>) -> Result<(StatusCode, Json<Value>), ApiError> {
    auth.web()?;
    let (name, tag) = (b.name.trim().to_string(), b.tag.trim().to_string());
    if !text(&name, 2, 32, false) {
        return Err(ApiError::bad("invalid_name"));
    }
    if !(2..=5).contains(&tag.len()) || !tag.chars().all(|c| c.is_ascii_alphanumeric()) {
        return Err(ApiError::bad("invalid_tag"));
    }
    let (id, now) = (auth.account.id, s.now());
    match s.db(move |c| make_team(c, id, &name, &tag, now)).await? {
        Ok(team) => Ok((StatusCode::CREATED, Json(json!({ "id": team })))),
        Err(code) => Err(ApiError::conflict(code)),
    }
}

async fn team_delete(State(s): State<AppState>, auth: Auth, Path(team): Path<i64>) -> Result<StatusCode, ApiError> {
    auth.web()?;
    let me = auth.account.id;
    let done = s
        .db(move |c| {
            let role: Option<String> = c.query_row("SELECT role FROM team_members WHERE team_id = ?1 AND account_id = ?2", [team, me], |r| r.get(0)).ok();
            if role.as_deref() != Some("leader") {
                return Ok(false);
            }
            c.execute("DELETE FROM teams WHERE id = ?1", [team])?;
            Ok(true)
        })
        .await?;
    if done { Ok(StatusCode::NO_CONTENT) } else { Err(ApiError::forbidden("forbidden")) }
}

async fn team_leave(State(s): State<AppState>, auth: Auth, Path(team): Path<i64>) -> Result<StatusCode, ApiError> {
    auth.web()?;
    let me = auth.account.id;
    let n = s.db(move |c| Ok(c.execute("DELETE FROM team_members WHERE team_id = ?1 AND account_id = ?2 AND role <> 'leader'", [team, me])?)).await?;
    if n == 1 { Ok(StatusCode::NO_CONTENT) } else { Err(ApiError::forbidden("forbidden")) }
}

// ---- support tickets ----------------------------------------------------------------------

const TICKET_CATEGORIES: [&str; 4] = ["launcher", "account", "in_game", "player_report"];

async fn tickets_list(State(s): State<AppState>, auth: Auth) -> Result<Json<Value>, ApiError> {
    auth.web()?;
    let id = auth.account.id;
    let v = s
        .db(move |c| {
            let mut st = c.prepare("SELECT id, category, subject, body, status, reply, created_at FROM support_tickets WHERE account_id = ?1 ORDER BY id DESC LIMIT 50")?;
            Ok(st
                .query_map([id], |r| {
                    Ok(json!({ "id": r.get::<_, i64>(0)?, "category": r.get::<_, String>(1)?, "subject": r.get::<_, String>(2)?, "body": r.get::<_, String>(3)?,
                               "status": r.get::<_, String>(4)?, "reply": r.get::<_, Option<String>>(5)?, "created_at": r.get::<_, i64>(6)? }))
                })?
                .collect::<Result<Vec<_>, _>>()?)
        })
        .await?;
    Ok(Json(json!({ "tickets": v })))
}

#[derive(Deserialize)]
struct TicketBody {
    category: String,
    subject: String,
    body: String,
}

async fn ticket_create(State(s): State<AppState>, auth: Auth, ip: ClientIp, Json(b): Json<TicketBody>) -> Result<(StatusCode, Json<Value>), ApiError> {
    auth.web()?;
    rate(&s, &ip, "ticket", 10, 3600)?;
    let (subject, body) = (b.subject.trim().to_string(), b.body.trim().to_string());
    if !TICKET_CATEGORIES.contains(&b.category.as_str()) || !text(&subject, 1, 120, false) || !text(&body, 1, 4000, true) {
        return Err(ApiError::bad("invalid_input"));
    }
    let (id, now) = (auth.account.id, s.now());
    let created = s
        .db(move |c| {
            let open: i64 = c.query_row("SELECT COUNT(*) FROM support_tickets WHERE account_id = ?1 AND status = 'open'", [id], |r| r.get(0))?;
            if open >= 5 {
                return Ok(None);
            }
            c.execute(
                "INSERT INTO support_tickets (account_id, category, subject, body, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                params![id, b.category, subject, body, now],
            )?;
            Ok(Some(c.last_insert_rowid()))
        })
        .await?;
    match created {
        Some(t) => Ok((StatusCode::CREATED, Json(json!({ "id": t })))),
        None => Err(ApiError::limited()),
    }
}

// ---- creator requests ------------------------------------------------------------------------

#[derive(Deserialize)]
struct CreatorBody {
    #[serde(default)]
    twitch: String,
    #[serde(default)]
    youtube: String,
    #[serde(default)]
    x: String,
    #[serde(default)]
    kind: String,
    #[serde(default)]
    title: String,
    #[serde(default)]
    message: String,
}

async fn creator_request(State(s): State<AppState>, auth: Auth, ip: ClientIp, Json(b): Json<CreatorBody>) -> Result<StatusCode, ApiError> {
    auth.web()?;
    rate(&s, &ip, "creator", 5, 86_400)?;
    let ok = ["", "skin", "event", "league", "other"].contains(&b.kind.as_str())
        && [&b.twitch, &b.youtube, &b.x].iter().all(|t| text(t, 0, 1024, false))
        && text(&b.title, 0, 120, false)
        && text(&b.message, 0, 4000, true);
    if !ok {
        return Err(ApiError::bad("invalid_input"));
    }
    let (id, now) = (auth.account.id, s.now());
    s.db(move |c| {
        c.execute(
            "INSERT INTO creator_requests (account_id, twitch, youtube, x, kind, title, message, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id, b.twitch.trim(), b.youtube.trim(), b.x.trim(), b.kind, b.title.trim(), b.message.trim(), now],
        )?;
        Ok(())
    })
    .await?;
    Ok(StatusCode::CREATED)
}

// ---- stream overlay ---------------------------------------------------------------------------------

fn overlay_values(c: &Connection, acct: i64) -> Result<Value, DbErr> {
    let row = board(c, &Board { region: None, mode: None, since: 0, q: None, account: Some(acct), limit: 1 })?.into_iter().next();
    let mut m = Map::new();
    let n = |v: &Value, k: &str| v[k].as_i64().unwrap_or(0);
    match row {
        Some(r) => {
            let (kills, deaths, games, wins) = (n(&r, "kills"), n(&r, "deaths"), n(&r, "games"), n(&r, "wins"));
            m.insert("rank".into(), json!(format!("#{}", n(&r, "rank"))));
            m.insert("score".into(), json!(n(&r, "score")));
            m.insert("kills".into(), json!(kills));
            m.insert("wins".into(), json!(wins));
            m.insert("kd".into(), json!(format!("{:.2}", if deaths > 0 { kills as f64 / deaths as f64 } else { kills as f64 })));
            m.insert("winrate".into(), json!(format!("{:.1}%", if games > 0 { wins as f64 * 100.0 / games as f64 } else { 0.0 })));
            m.insert("games".into(), json!(games));
            m.insert("deaths".into(), json!(deaths));
            m.insert("best".into(), json!(n(&r, "best")));
        }
        None => {
            for f in OVERLAY_FIELDS {
                m.insert(f.into(), json!(if f == "rank" { "—" } else { "0" }));
            }
        }
    }
    Ok(Value::Object(m))
}

fn ensure_overlay(c: &Connection, acct: i64) -> Result<(String, Vec<String>), DbErr> {
    let existing = c.query_row("SELECT token, fields FROM overlays WHERE account_id = ?1", [acct], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).ok();
    let (token, fields) = match existing {
        Some(x) => x,
        None => {
            let token = random_token();
            let fields = json!(["rank", "score", "kills", "wins"]).to_string();
            c.execute("INSERT INTO overlays (account_id, token, fields) VALUES (?1, ?2, ?3)", params![acct, token, fields])?;
            (token, fields)
        }
    };
    Ok((token, serde_json::from_str(&fields).unwrap_or_else(|_| Vec::new())))
}

async fn overlay_get(State(s): State<AppState>, auth: Auth) -> Result<Json<Value>, ApiError> {
    auth.web()?;
    let id = auth.account.id;
    let (token, fields, sample) = s
        .db(move |c| {
            let (t, f) = ensure_overlay(c, id)?;
            Ok((t, f, overlay_values(c, id)?))
        })
        .await?;
    Ok(Json(json!({ "token": token, "fields": fields, "sample": sample })))
}

#[derive(Deserialize)]
struct OverlayBody {
    fields: Vec<String>,
}

async fn overlay_put(State(s): State<AppState>, auth: Auth, Json(b): Json<OverlayBody>) -> Result<StatusCode, ApiError> {
    auth.web()?;
    let mut seen = std::collections::HashSet::new();
    if b.fields.len() > 5 || !b.fields.iter().all(|f| OVERLAY_FIELDS.contains(&f.as_str()) && seen.insert(f.clone())) {
        return Err(ApiError::bad("invalid_input"));
    }
    let id = auth.account.id;
    let fields = json!(b.fields).to_string();
    s.db(move |c| {
        ensure_overlay(c, id)?;
        c.execute("UPDATE overlays SET fields = ?1 WHERE account_id = ?2", params![fields, id])?;
        Ok(())
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn overlay_regen(State(s): State<AppState>, auth: Auth) -> Result<Json<Value>, ApiError> {
    auth.web()?;
    let id = auth.account.id;
    let token = random_token();
    let t2 = token.clone();
    s.db(move |c| {
        ensure_overlay(c, id)?;
        c.execute("UPDATE overlays SET token = ?1 WHERE account_id = ?2", params![t2, id])?;
        Ok(())
    })
    .await?;
    Ok(Json(json!({ "token": token })))
}

/// What OBS loads. The link is the credential and shows only public stats.
async fn overlay_public(State(s): State<AppState>, ip: ClientIp, Path(token): Path<String>) -> Result<Json<Value>, ApiError> {
    rate(&s, &ip, "overlay", 120, 60)?;
    if token.len() > 64 {
        return Err(ApiError::not_found("not_found"));
    }
    let found = s
        .db(move |c| {
            let row = c
                .query_row(
                    "SELECT o.account_id, o.fields, a.callsign FROM overlays o JOIN accounts a ON a.id = o.account_id WHERE o.token = ?1 AND a.banned = 0",
                    [token],
                    |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)),
                )
                .ok();
            match row {
                Some((id, fields, callsign)) => Ok(Some((callsign, fields, overlay_values(c, id)?))),
                None => Ok(None),
            }
        })
        .await?;
    let (callsign, fields, values) = found.ok_or(ApiError::not_found("not_found"))?;
    let fields: Value = serde_json::from_str(&fields).unwrap_or(json!([]));
    Ok(Json(json!({ "callsign": callsign, "fields": fields, "values": values })))
}

// ---- competitions --------------------------------------------------------------------------------------

#[derive(Deserialize)]
struct KindQuery {
    kind: Option<String>,
}

async fn competitions(State(s): State<AppState>, Query(q): Query<KindQuery>) -> Result<Json<Value>, ApiError> {
    let kind = q.kind.filter(|k| k == "league" || k == "tournament");
    let rows = s
        .db(move |c| {
            let mut st = c.prepare("SELECT id, kind, title, creator, mode, region, status FROM competitions WHERE (?1 IS NULL OR kind = ?1) ORDER BY id DESC LIMIT 100")?;
            Ok(st
                .query_map([kind], |r| {
                    Ok(json!({ "id": r.get::<_, i64>(0)?, "kind": r.get::<_, String>(1)?, "title": r.get::<_, String>(2)?, "creator": r.get::<_, String>(3)?,
                               "mode": r.get::<_, String>(4)?, "region": r.get::<_, String>(5)?, "status": r.get::<_, String>(6)? }))
                })?
                .collect::<Result<Vec<_>, _>>()?)
        })
        .await?;
    Ok(Json(json!({ "competitions": rows })))
}

// ---- admin ------------------------------------------------------------------------------------------------

#[derive(Deserialize)]
struct StatusQuery {
    status: Option<String>,
}

async fn admin_tickets(State(s): State<AppState>, _a: AdminAuth, Query(q): Query<StatusQuery>) -> Result<Json<Value>, ApiError> {
    let status = q.status.filter(|t| t == "open" || t == "closed");
    let rows = s
        .db(move |c| {
            let mut st = c.prepare(
                "SELECT t.id, t.account_id, a.callsign, t.category, t.subject, t.body, t.status, t.reply, t.created_at FROM support_tickets t \
                 JOIN accounts a ON a.id = t.account_id WHERE (?1 IS NULL OR t.status = ?1) ORDER BY t.id LIMIT 200",
            )?;
            Ok(st
                .query_map([status], |r| {
                    Ok(json!({ "id": r.get::<_, i64>(0)?, "account_id": r.get::<_, i64>(1)?, "callsign": r.get::<_, String>(2)?, "category": r.get::<_, String>(3)?,
                               "subject": r.get::<_, String>(4)?, "body": r.get::<_, String>(5)?, "status": r.get::<_, String>(6)?,
                               "reply": r.get::<_, Option<String>>(7)?, "created_at": r.get::<_, i64>(8)? }))
                })?
                .collect::<Result<Vec<_>, _>>()?)
        })
        .await?;
    Ok(Json(json!({ "tickets": rows })))
}

#[derive(Deserialize)]
struct ReplyBody {
    reply: String,
    #[serde(default)]
    close: bool,
}

async fn admin_ticket_reply(State(s): State<AppState>, _a: AdminAuth, Path(id): Path<i64>, Json(b): Json<ReplyBody>) -> Result<StatusCode, ApiError> {
    if !text(&b.reply, 1, 4000, true) {
        return Err(ApiError::bad("invalid_input"));
    }
    let now = s.now();
    let n = s
        .db(move |c| {
            Ok(c.execute(
                "UPDATE support_tickets SET reply = ?1, status = CASE WHEN ?2 THEN 'closed' ELSE status END, updated_at = ?3 WHERE id = ?4",
                params![b.reply, b.close, now, id],
            )?)
        })
        .await?;
    if n == 1 { Ok(StatusCode::NO_CONTENT) } else { Err(ApiError::not_found("not_found")) }
}

async fn admin_creator_requests(State(s): State<AppState>, _a: AdminAuth) -> Result<Json<Value>, ApiError> {
    let rows = s
        .db(move |c| {
            let mut st = c.prepare(
                "SELECT r.id, a.callsign, r.twitch, r.youtube, r.x, r.kind, r.title, r.message, r.created_at FROM creator_requests r \
                 JOIN accounts a ON a.id = r.account_id ORDER BY r.id DESC LIMIT 200",
            )?;
            Ok(st
                .query_map([], |r| {
                    Ok(json!({ "id": r.get::<_, i64>(0)?, "callsign": r.get::<_, String>(1)?, "twitch": r.get::<_, String>(2)?, "youtube": r.get::<_, String>(3)?,
                               "x": r.get::<_, String>(4)?, "kind": r.get::<_, String>(5)?, "title": r.get::<_, String>(6)?, "message": r.get::<_, String>(7)?, "created_at": r.get::<_, i64>(8)? }))
                })?
                .collect::<Result<Vec<_>, _>>()?)
        })
        .await?;
    Ok(Json(json!({ "requests": rows })))
}

#[derive(Deserialize)]
struct CompetitionBody {
    kind: String,
    title: String,
    creator: String,
    mode: String,
    region: String,
    status: String,
}

async fn admin_competition(State(s): State<AppState>, _a: AdminAuth, Json(b): Json<CompetitionBody>) -> Result<(StatusCode, Json<Value>), ApiError> {
    let ok = (b.kind == "league" || b.kind == "tournament")
        && text(&b.title, 1, 80, false)
        && text(&b.creator, 1, 40, false)
        && text(&b.mode, 1, 20, false)
        && REGIONS.contains(&b.region.as_str())
        && ["applications", "running", "finalizing", "completed", "cancelled"].contains(&b.status.as_str());
    if !ok {
        return Err(ApiError::bad("invalid_input"));
    }
    let now = s.now();
    let id = s
        .db(move |c| {
            c.execute(
                "INSERT INTO competitions (kind, title, creator, mode, region, status, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![b.kind, b.title, b.creator, b.mode, b.region, b.status, now],
            )?;
            Ok(c.last_insert_rowid())
        })
        .await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
}

#[derive(Deserialize)]
struct BanBody {
    banned: bool,
}

async fn admin_ban(State(s): State<AppState>, _a: AdminAuth, Path(id): Path<i64>, Json(b): Json<BanBody>) -> Result<StatusCode, ApiError> {
    let n = s
        .db(move |c| {
            let n = c.execute("UPDATE accounts SET banned = ?1 WHERE id = ?2", params![b.banned, id])?;
            if b.banned {
                c.execute("DELETE FROM sessions WHERE account_id = ?1", [id])?;
                c.execute("DELETE FROM tickets WHERE account_id = ?1", [id])?;
            }
            Ok(n)
        })
        .await?;
    if n == 1 { Ok(StatusCode::NO_CONTENT) } else { Err(ApiError::not_found("not_found")) }
}
