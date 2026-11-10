//! HTTP routes.

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::Path;

use axum::{
    Json, Router,
    extract::{ConnectInfo, DefaultBodyLimit, FromRequestParts, Query, Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, header, request::Parts},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    AppState,
    crypto::{self, random_below, random_launcher_key, random_token, sha256_hex},
    db::{self, Account, PlayerResult},
    error::ApiError,
    steam,
};

const TICKET_SECONDS: i64 = 60;
const CODE_SECONDS: i64 = 120;
const STATE_SECONDS: i64 = 600;

pub fn router(state: AppState) -> Router {
    let mut r = Router::new()
        .route("/health", get(|| async { "ok" }))
        .route("/register", post(register))
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/me", get(me))
        .route("/profile", get(get_profile).put(put_profile))
        .route("/password", post(set_password))
        .route("/leaderboard", get(crate::site::leaderboard))
        .route("/api-key", get(key_info).post(key_create).delete(key_delete))
        .route("/auth/steam/start", get(steam_start))
        .route("/auth/steam/link", post(steam_link))
        .route("/auth/steam/callback", get(steam_callback))
        .route("/auth/steam", delete(steam_unlink))
        .route("/auth/exchange", post(exchange))
        .route("/join-ticket", post(join_ticket))
        .route("/verify", post(verify))
        .route("/results", post(results))
        .merge(crate::site::routes());
    if state.cfg.static_dir.is_some() {
        r = r.fallback(static_files);
    }
    r.layer(DefaultBodyLimit::max(64 * 1024))
        .layer(middleware::from_fn_with_state(state.clone(), cors))
        .with_state(state)
}

// ---- extractors ---------------------------------------------------------------

pub struct ClientIp(pub String);

impl FromRequestParts<AppState> for ClientIp {
    type Rejection = Infallible;
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Infallible> {
        if let Some(name) = &state.cfg.trust_ip_header
            && let Some(v) = parts.headers.get(name.as_str()).and_then(|v| v.to_str().ok())
        {
            let v = v.trim();
            if !v.is_empty() && v.len() <= 64 {
                return Ok(ClientIp(v.to_string()));
            }
        }
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|c| c.0.ip().to_string())
            .unwrap_or_else(|| "local".into());
        Ok(ClientIp(peer))
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Via {
    Web,
    Key,
}

/// The signed-in account, from `Authorization: Bearer <session token or launcher key>`.
pub struct Auth {
    pub account: Account,
    pub token_hash: String,
    pub via: Via,
}

impl Auth {
    /// Some actions (keys, password, Steam link) need a website session, not a launcher key.
    pub(crate) fn web(&self) -> Result<(), ApiError> {
        if self.via == Via::Web { Ok(()) } else { Err(ApiError::forbidden("web_session_required")) }
    }
}

fn bearer(h: &HeaderMap) -> Option<String> {
    let v = h.get(header::AUTHORIZATION)?.to_str().ok()?;
    let t = v.strip_prefix("Bearer ")?.trim();
    (!t.is_empty() && t.len() <= 128).then(|| t.to_string())
}

impl FromRequestParts<AppState> for Auth {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let token = bearer(&parts.headers).ok_or_else(ApiError::unauthorized)?;
        let now = state.now();
        let (account, token_hash, via) = if crypto::is_launcher_key(&token) {
            let hash = sha256_hex(&token.to_ascii_lowercase());
            let h = hash.clone();
            (state.db(move |c| db::key_account(c, &h, now)).await?, hash, Via::Key)
        } else {
            let hash = sha256_hex(&token);
            let h = hash.clone();
            (state.db(move |c| db::session_account(c, &h, now)).await?, hash, Via::Web)
        };
        let account = account.ok_or_else(ApiError::unauthorized)?;
        if account.banned {
            return Err(ApiError::forbidden("banned"));
        }
        Ok(Auth { account, token_hash, via })
    }
}

/// A game server, from the `X-Server-Key` header.
pub struct ServerAuth(pub String);

impl FromRequestParts<AppState> for ServerAuth {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, ApiError> {
        let key = parts.headers.get("x-server-key").and_then(|v| v.to_str().ok()).ok_or_else(ApiError::unauthorized)?;
        let hash = sha256_hex(key.trim());
        state
            .cfg
            .server_keys
            .iter()
            .find(|(_, h)| *h == hash)
            .map(|(n, _)| ServerAuth(n.clone()))
            .ok_or_else(ApiError::unauthorized)
    }
}

// ---- views ----------------------------------------------------------------------

#[derive(Serialize)]
struct Stats {
    kills: i64,
    deaths: i64,
    score: i64,
    wins: i64,
    matches: i64,
}

#[derive(Serialize)]
struct AccountView {
    id: i64,
    username: String,
    callsign: String,
    emblem: String,
    steam_id: Option<String>,
    has_password: bool,
    created_at: i64,
    stats: Stats,
}

fn view(a: &Account) -> AccountView {
    AccountView {
        id: a.id,
        username: a.username.clone(),
        callsign: a.callsign.clone(),
        emblem: a.emblem.clone(),
        steam_id: a.steam_id.clone(),
        has_password: a.password_hash.is_some(),
        created_at: a.created_at,
        stats: Stats { kills: a.kills, deaths: a.deaths, score: a.score, wins: a.wins, matches: a.matches },
    }
}

#[derive(Serialize)]
struct AuthResponse {
    token: String,
    expires_at: i64,
    account: AccountView,
}

async fn issue_session(s: &AppState, account_id: i64) -> Result<(String, i64), ApiError> {
    let token = random_token();
    let hash = sha256_hex(&token);
    let now = s.now();
    let expires = now + s.cfg.session_days * 86_400;
    s.db(move |c| db::insert_session(c, &hash, account_id, now, expires)).await?;
    Ok((token, expires))
}

async fn auth_response(s: &AppState, account_id: i64) -> Result<Json<AuthResponse>, ApiError> {
    let (token, expires_at) = issue_session(s, account_id).await?;
    let account = s.db(move |c| db::account_by_id(c, account_id)).await?.ok_or_else(ApiError::internal)?;
    Ok(Json(AuthResponse { token, expires_at, account: view(&account) }))
}

// ---- validation -------------------------------------------------------------------

fn valid_username(s: &str) -> bool {
    (3..=20).contains(&s.len())
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        && !s.to_ascii_lowercase().starts_with("steam_")
}

fn valid_password(s: &str) -> bool {
    (8..=128).contains(&s.len())
}

fn clean_callsign(s: &str) -> Option<String> {
    let t = s.trim();
    let ok = (2..=16).contains(&t.len())
        && t.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b' ' | b'_' | b'-' | b'.'));
    ok.then(|| t.to_string())
}

async fn hash_password(s: &AppState, password: String) -> Result<String, ApiError> {
    let hasher = s.hasher.clone();
    tokio::task::spawn_blocking(move || hasher.hash(&password))
        .await
        .map_err(|_| ApiError::internal())?
        .ok_or_else(ApiError::internal)
}

async fn verify_password(s: &AppState, stored: Option<String>, password: String) -> Result<bool, ApiError> {
    let hasher = s.hasher.clone();
    tokio::task::spawn_blocking(move || hasher.verify(stored.as_deref(), &password))
        .await
        .map_err(|_| ApiError::internal())
}

pub(crate) fn rate(s: &AppState, ip: &ClientIp, what: &str, max: usize, window: i64) -> Result<(), ApiError> {
    if s.limiter.allow(&format!("{what}:{}", ip.0), max, window, s.now()) { Ok(()) } else { Err(ApiError::limited()) }
}

// ---- password accounts --------------------------------------------------------------

#[derive(Deserialize)]
struct Creds {
    username: String,
    password: String,
}

async fn register(State(s): State<AppState>, ip: ClientIp, Json(b): Json<Creds>) -> Result<Json<AuthResponse>, ApiError> {
    rate(&s, &ip, "reg", 5, 3600)?;
    if !valid_username(&b.username) {
        return Err(ApiError::bad("invalid_username"));
    }
    if !valid_password(&b.password) {
        return Err(ApiError::bad("invalid_password"));
    }
    let hash = hash_password(&s, b.password).await?;
    let callsign: String = b.username.chars().take(16).collect();
    let now = s.now();
    let name = b.username;
    let id = s
        .db(move |c| db::create_account(c, &name, Some(&hash), None, &callsign, now))
        .await
        .map_err(|e| e.unique_or("username_taken"))?;
    auth_response(&s, id).await
}

async fn login(State(s): State<AppState>, ip: ClientIp, Json(b): Json<Creds>) -> Result<Json<AuthResponse>, ApiError> {
    rate(&s, &ip, "auth", 20, 60)?;
    let now = s.now();
    let fail_key = format!("lf:{}", b.username.to_ascii_lowercase());
    if s.limiter.blocked(&fail_key, 5, 300, now) {
        return Err(ApiError::limited());
    }
    let name = b.username.clone();
    let acc = s.db(move |c| db::account_by_username(c, &name)).await?;
    let ok = verify_password(&s, acc.as_ref().and_then(|a| a.password_hash.clone()), b.password).await?;
    match acc {
        Some(a) if ok => {
            if a.banned {
                return Err(ApiError::forbidden("banned"));
            }
            auth_response(&s, a.id).await
        }
        _ => {
            s.limiter.record(&fail_key, now);
            Err(ApiError::new(StatusCode::UNAUTHORIZED, "invalid_credentials"))
        }
    }
}

async fn logout(State(s): State<AppState>, auth: Auth) -> Result<StatusCode, ApiError> {
    auth.web()?;
    let h = auth.token_hash;
    s.db(move |c| db::delete_session(c, &h)).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct PasswordBody {
    current: Option<String>,
    new: String,
}

async fn set_password(State(s): State<AppState>, auth: Auth, Json(b): Json<PasswordBody>) -> Result<StatusCode, ApiError> {
    auth.web()?;
    if !valid_password(&b.new) {
        return Err(ApiError::bad("invalid_password"));
    }
    let id = auth.account.id;
    if let Some(stored) = auth.account.password_hash.clone() {
        let key = format!("pw:{id}");
        let now = s.now();
        if s.limiter.blocked(&key, 5, 300, now) {
            return Err(ApiError::limited());
        }
        let ok = verify_password(&s, Some(stored), b.current.unwrap_or_else(String::new)).await?;
        if !ok {
            s.limiter.record(&key, now);
            return Err(ApiError::new(StatusCode::UNAUTHORIZED, "invalid_credentials"));
        }
    }
    let hash = hash_password(&s, b.new).await?;
    let keep = auth.token_hash;
    s.db(move |c| {
        db::set_password(c, id, &hash)?;
        db::delete_other_sessions(c, id, &keep)
    })
    .await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---- profile --------------------------------------------------------------------------

async fn me(auth: Auth) -> Json<AccountView> {
    Json(view(&auth.account))
}

#[derive(Serialize)]
struct ProfileView {
    account: AccountView,
    classes: Value,
    settings: Value,
}

async fn get_profile(auth: Auth) -> Json<ProfileView> {
    let a = &auth.account;
    Json(ProfileView {
        account: view(a),
        classes: serde_json::from_str(&a.classes).unwrap_or(Value::Array(vec![])),
        settings: serde_json::from_str(&a.settings).unwrap_or(Value::Object(Default::default())),
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileUpdate {
    callsign: Option<String>,
    emblem: Option<String>,
    classes: Option<Value>,
    settings: Option<Value>,
}

async fn put_profile(State(s): State<AppState>, auth: Auth, Json(b): Json<ProfileUpdate>) -> Result<Json<AccountView>, ApiError> {
    let callsign = match &b.callsign {
        Some(c) => Some(clean_callsign(c).ok_or(ApiError::bad("invalid_callsign"))?),
        None => None,
    };
    let emblem = match b.emblem {
        Some(e) if e.len() <= 64 && e.chars().all(|c| !c.is_control()) => Some(e),
        Some(_) => return Err(ApiError::bad("invalid_emblem")),
        None => None,
    };
    let classes = match b.classes {
        Some(v) if v.is_array() || v.is_object() => {
            let t = v.to_string();
            if t.len() > 32 * 1024 {
                return Err(ApiError::bad("classes_too_large"));
            }
            Some(t)
        }
        Some(_) => return Err(ApiError::bad("invalid_classes")),
        None => None,
    };
    let settings = match b.settings {
        Some(v) if v.is_object() => {
            let t = v.to_string();
            if t.len() > 16 * 1024 {
                return Err(ApiError::bad("settings_too_large"));
            }
            Some(t)
        }
        Some(_) => return Err(ApiError::bad("invalid_settings")),
        None => None,
    };
    let id = auth.account.id;
    let updated = s
        .db(move |c| {
            db::update_profile(c, id, callsign.as_deref(), emblem.as_deref(), classes.as_deref(), settings.as_deref())?;
            db::account_by_id(c, id)
        })
        .await?
        .ok_or_else(ApiError::internal)?;
    Ok(Json(view(&updated)))
}

// ---- launcher key -----------------------------------------------------------------------

async fn key_info(State(s): State<AppState>, auth: Auth) -> Result<Json<Value>, ApiError> {
    auth.web()?;
    let id = auth.account.id;
    let info = s.db(move |c| db::key_info(c, id)).await?;
    Ok(Json(match info {
        Some((created, last)) => serde_json::json!({ "exists": true, "created_at": created, "last_used": last }),
        None => serde_json::json!({ "exists": false }),
    }))
}

/// Make a new launcher key (replacing any old one). The key is shown once.
async fn key_create(State(s): State<AppState>, auth: Auth) -> Result<Json<Value>, ApiError> {
    auth.web()?;
    let key = random_launcher_key();
    let hash = sha256_hex(&key);
    let id = auth.account.id;
    let now = s.now();
    s.db(move |c| db::put_key(c, id, &hash, now)).await?;
    Ok(Json(serde_json::json!({ "key": key, "created_at": now })))
}

async fn key_delete(State(s): State<AppState>, auth: Auth) -> Result<StatusCode, ApiError> {
    auth.web()?;
    let id = auth.account.id;
    s.db(move |c| db::delete_key(c, id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---- Steam -----------------------------------------------------------------------------

fn with_query(base: &str, pairs: &[(&str, &str)]) -> String {
    match url::Url::parse(base) {
        Ok(mut u) => {
            u.query_pairs_mut().extend_pairs(pairs.iter().copied());
            u.to_string()
        }
        Err(_) => base.to_string(),
    }
}

#[derive(Deserialize)]
struct StartQuery {
    #[serde(rename = "return")]
    return_to: String,
}

async fn begin_steam(s: &AppState, purpose: &'static str, account_id: Option<i64>, return_to: String) -> Result<String, ApiError> {
    if !s.cfg.return_allowed(&return_to) {
        return Err(ApiError::bad("return_not_allowed"));
    }
    let state = random_token();
    let st = state.clone();
    let expires = s.now() + STATE_SECONDS;
    s.db(move |c| db::insert_state(c, &st, purpose, account_id, &return_to, expires)).await?;
    Ok(steam::auth_url(&s.cfg.public_url, &state))
}

/// Start a Steam sign-in. The browser is sent to Steam and, afterwards, back to
/// `return` with `?code=...` (exchange it at `POST /auth/exchange`) or `?error=...`.
async fn steam_start(State(s): State<AppState>, ip: ClientIp, Query(q): Query<StartQuery>) -> Result<Redirect, ApiError> {
    rate(&s, &ip, "steam", 30, 60)?;
    let url = begin_steam(&s, "login", None, q.return_to).await?;
    Ok(Redirect::to(&url))
}

#[derive(Deserialize)]
struct LinkBody {
    #[serde(rename = "return")]
    return_to: String,
}

/// Start linking Steam to the signed-in account. Returns the URL to open.
async fn steam_link(State(s): State<AppState>, auth: Auth, Json(b): Json<LinkBody>) -> Result<Json<Value>, ApiError> {
    auth.web()?;
    let url = begin_steam(&s, "link", Some(auth.account.id), b.return_to).await?;
    Ok(Json(serde_json::json!({ "url": url })))
}

enum Outcome {
    Code(String),
    Linked,
}

async fn finish_steam(s: &AppState, q: &HashMap<String, String>, state: &str, st: &db::SteamState) -> Result<Outcome, &'static str> {
    let expected = steam::return_to(&s.cfg.public_url, state);
    let assertion = steam::check_local(q, &expected)?;
    let now = s.now();
    let nonce = assertion.nonce.clone();
    if !s.db(move |c| db::claim_nonce(c, &nonce, now)).await.map_err(|_| "internal")? {
        return Err("steam_replay");
    }
    let form = steam::check_form(q);
    let verifier = s.steam.clone();
    match tokio::task::spawn_blocking(move || verifier.check_authentication(&form)).await {
        Ok(Ok(true)) => {}
        Ok(Ok(false)) => return Err("steam_rejected"),
        Ok(Err(e)) => {
            eprintln!("accounts: could not confirm a Steam sign-in with Steam: {e}");
            return Err("steam_unreachable");
        }
        Err(_) => return Err("steam_unreachable"),
    }
    let steam_id = assertion.steam_id;
    if st.purpose == "link" {
        let account_id = st.account_id.ok_or("internal")?;
        let sid = steam_id.clone();
        return match s.db(move |c| db::set_steam(c, account_id, &sid)).await {
            Ok(()) => Ok(Outcome::Linked),
            Err(crate::DbErr::Unique) => Err("steam_in_use"),
            Err(_) => Err("internal"),
        };
    }
    let sid = steam_id.clone();
    let existing = s.db(move |c| db::account_by_steam(c, &sid)).await.map_err(|_| "internal")?;
    let account = match existing {
        Some(a) => a,
        None => {
            let username = format!("steam_{steam_id}");
            let callsign = format!("Player{:04}", random_below(10_000));
            let sid = steam_id.clone();
            let created = s.db(move |c| db::create_account(c, &username, None, Some(&sid), &callsign, now)).await;
            let id = match created {
                Ok(id) => id,
                // Two sign-ins at once: the other one won.
                Err(crate::DbErr::Unique) => {
                    let sid = steam_id.clone();
                    s.db(move |c| db::account_by_steam(c, &sid)).await.map_err(|_| "internal")?.ok_or("internal")?.id
                }
                Err(_) => return Err("internal"),
            };
            s.db(move |c| db::account_by_id(c, id)).await.map_err(|_| "internal")?.ok_or("internal")?
        }
    };
    if account.banned {
        return Err("banned");
    }
    let code = random_token();
    let hash = sha256_hex(&code);
    let id = account.id;
    let expires = now + CODE_SECONDS;
    s.db(move |c| db::insert_code(c, &hash, id, expires)).await.map_err(|_| "internal")?;
    Ok(Outcome::Code(code))
}

async fn steam_callback(State(s): State<AppState>, ip: ClientIp, Query(q): Query<HashMap<String, String>>) -> Response {
    if rate(&s, &ip, "steamcb", 30, 60).is_err() {
        return ApiError::limited().into_response();
    }
    let Some(state) = q.get("state").cloned() else {
        return ApiError::bad("missing_state").into_response();
    };
    let now = s.now();
    let st_key = state.clone();
    let st = match s.db(move |c| db::take_state(c, &st_key, now)).await {
        Ok(Some(st)) => st,
        Ok(None) => return ApiError::bad("unknown_state").into_response(),
        Err(_) => return ApiError::internal().into_response(),
    };
    // From here the return address is trusted (it was checked when sign-in started),
    // so failures are sent back to the page that asked.
    let target = match finish_steam(&s, &q, &state, &st).await {
        Ok(Outcome::Code(code)) => with_query(&st.return_to, &[("code", &code)]),
        Ok(Outcome::Linked) => with_query(&st.return_to, &[("linked", "1")]),
        Err(code) => with_query(&st.return_to, &[("error", code)]),
    };
    Redirect::to(&target).into_response()
}

#[derive(Deserialize)]
struct ExchangeBody {
    code: String,
}

/// Trade the one-time code from a Steam sign-in for a website session.
async fn exchange(State(s): State<AppState>, ip: ClientIp, Json(b): Json<ExchangeBody>) -> Result<Json<AuthResponse>, ApiError> {
    rate(&s, &ip, "auth", 20, 60)?;
    let hash = sha256_hex(b.code.trim());
    let now = s.now();
    let acc = s.db(move |c| db::take_code(c, &hash, now)).await?.ok_or(ApiError::new(StatusCode::UNAUTHORIZED, "invalid_code"))?;
    if acc.banned {
        return Err(ApiError::forbidden("banned"));
    }
    auth_response(&s, acc.id).await
}

/// Unlink Steam. Refused if it is the only way into the account.
async fn steam_unlink(State(s): State<AppState>, auth: Auth) -> Result<StatusCode, ApiError> {
    auth.web()?;
    if auth.account.password_hash.is_none() {
        return Err(ApiError::conflict("set_password_first"));
    }
    let id = auth.account.id;
    s.db(move |c| db::clear_steam(c, id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---- joining a server ------------------------------------------------------------------

/// A single-use ticket, valid for a minute. The launcher passes it to the game,
/// which hands it to the server; the server never sees the launcher key.
async fn join_ticket(State(s): State<AppState>, auth: Auth) -> Result<Json<Value>, ApiError> {
    let id = auth.account.id;
    if !s.limiter.allow(&format!("jt:{id}"), 30, 60, s.now()) {
        return Err(ApiError::limited());
    }
    let ticket = random_token();
    let hash = sha256_hex(&ticket);
    let expires = s.now() + TICKET_SECONDS;
    s.db(move |c| db::insert_ticket(c, &hash, id, expires)).await?;
    Ok(Json(serde_json::json!({ "ticket": ticket, "expires_in": TICKET_SECONDS })))
}

#[derive(Deserialize)]
struct VerifyBody {
    ticket: String,
}

async fn verify(State(s): State<AppState>, _server: ServerAuth, Json(b): Json<VerifyBody>) -> Result<Json<Value>, ApiError> {
    let hash = sha256_hex(b.ticket.trim());
    let now = s.now();
    let acc = s.db(move |c| db::take_ticket(c, &hash, now)).await?.ok_or(ApiError::not_found("invalid_ticket"))?;
    if acc.banned {
        return Err(ApiError::forbidden("banned"));
    }
    Ok(Json(serde_json::json!({ "account_id": acc.id, "callsign": acc.callsign, "steam_id": acc.steam_id })))
}

#[derive(Deserialize)]
struct ResultsBody {
    match_id: String,
    map: String,
    mode: String,
    region: Option<String>,
    duration_secs: Option<i64>,
    players: Vec<PlayerIn>,
}

#[derive(Deserialize)]
struct PlayerIn {
    account_id: i64,
    kills: i64,
    deaths: i64,
    score: i64,
    won: bool,
}

/// A game server reports a finished match. Reporting the same match twice is harmless.
async fn results(State(s): State<AppState>, server: ServerAuth, Json(b): Json<ResultsBody>) -> Result<Json<Value>, ApiError> {
    let short = |t: &str, max: usize| !t.is_empty() && t.len() <= max && t.chars().all(|c| !c.is_control());
    if !short(&b.match_id, 64) || !short(&b.map, 40) || !short(&b.mode, 20) || b.players.is_empty() || b.players.len() > 64 {
        return Err(ApiError::bad("invalid_results"));
    }
    let mut players = Vec::new();
    for p in &b.players {
        if !(0..=500).contains(&p.kills) || !(0..=500).contains(&p.deaths) || !(0..=1_000_000).contains(&p.score) {
            return Err(ApiError::bad("invalid_results"));
        }
        players.push(PlayerResult { account_id: p.account_id, kills: p.kills, deaths: p.deaths, score: p.score, won: p.won });
    }
    let region = b.region.clone().unwrap_or_else(|| "EU".into());
    if !crate::site::REGIONS.contains(&region.as_str()) {
        return Err(ApiError::bad("invalid_results"));
    }
    let duration = b.duration_secs.unwrap_or(0).clamp(0, 7200);
    let now = s.now();
    let name = server.0;
    let recorded =
        s.db(move |c| db::record_match(c, &name, &b.match_id, &b.map, &b.mode, &region, duration, &players, now)).await?;
    Ok(Json(serde_json::json!({ "recorded": recorded })))
}

// ---- CORS, security headers, static site ---------------------------------------------

async fn cors(State(s): State<AppState>, req: Request, next: Next) -> Response {
    let origin = req
        .headers()
        .get(header::ORIGIN)
        .and_then(|v| v.to_str().ok())
        .filter(|o| s.cfg.origin_allowed(o))
        .map(str::to_string);
    let preflight = req.method() == Method::OPTIONS;
    let mut resp = if preflight { StatusCode::NO_CONTENT.into_response() } else { next.run(req).await };
    let h = resp.headers_mut();
    if let Some(o) = origin.and_then(|o| HeaderValue::from_str(&o).ok()) {
        h.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, o);
        h.insert(header::ACCESS_CONTROL_ALLOW_HEADERS, HeaderValue::from_static("authorization, content-type"));
        h.insert(header::ACCESS_CONTROL_ALLOW_METHODS, HeaderValue::from_static("GET, POST, PUT, DELETE, OPTIONS"));
        h.insert(header::ACCESS_CONTROL_MAX_AGE, HeaderValue::from_static("600"));
    }
    h.append(header::VARY, HeaderValue::from_static("Origin"));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    if !h.contains_key(header::CACHE_CONTROL) {
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    resp
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Serves the website folder (local testing and single-host setups).
async fn static_files(State(s): State<AppState>, uri: Uri) -> Response {
    let Some(dir) = s.cfg.static_dir.clone() else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let rel = uri.path().trim_start_matches('/');
    let rel = if rel.is_empty() { "index.html" } else { rel };
    let safe = rel.split('/').all(|c| !c.is_empty() && !c.starts_with('.') && !c.contains('\\') && !c.contains(':'));
    if safe && let Ok(bytes) = tokio::fs::read(Path::new(&dir).join(rel)).await {
        return ([(header::CONTENT_TYPE, content_type(rel)), (header::CACHE_CONTROL, "no-cache")], bytes).into_response();
    }
    // The site is a single-page app: extensionless paths are client-side routes.
    if !rel.rsplit('/').next().is_some_and(|f| f.contains('.')) && let Ok(bytes) = tokio::fs::read(Path::new(&dir).join("index.html")).await {
        return ([(header::CONTENT_TYPE, "text/html; charset=utf-8"), (header::CACHE_CONTROL, "no-cache")], bytes).into_response();
    }
    match tokio::fs::read(Path::new(&dir).join("404.html")).await {
        Ok(bytes) => (StatusCode::NOT_FOUND, [(header::CONTENT_TYPE, "text/html; charset=utf-8")], bytes).into_response(),
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}
