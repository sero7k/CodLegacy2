//! SQLite storage. Every function takes a connection, so the service can run
//! them on a blocking thread and tests can call them directly.

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::error::DbErr;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS accounts (
    id            INTEGER PRIMARY KEY,
    username      TEXT NOT NULL UNIQUE COLLATE NOCASE,
    password_hash TEXT,
    steam_id      TEXT UNIQUE,
    callsign      TEXT NOT NULL,
    emblem        TEXT NOT NULL DEFAULT '',
    classes       TEXT NOT NULL DEFAULT '[]',
    settings      TEXT NOT NULL DEFAULT '{}',
    banned        INTEGER NOT NULL DEFAULT 0,
    kills         INTEGER NOT NULL DEFAULT 0,
    deaths        INTEGER NOT NULL DEFAULT 0,
    score         INTEGER NOT NULL DEFAULT 0,
    wins          INTEGER NOT NULL DEFAULT 0,
    matches       INTEGER NOT NULL DEFAULT 0,
    created_at    INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS sessions (
    token_hash TEXT PRIMARY KEY,
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS sessions_account ON sessions(account_id);
CREATE TABLE IF NOT EXISTS api_keys (
    account_id INTEGER PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    key_hash   TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL,
    last_used  INTEGER
);
CREATE TABLE IF NOT EXISTS tickets (
    token_hash TEXT PRIMARY KEY,
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    expires_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS exchange_codes (
    code_hash  TEXT PRIMARY KEY,
    account_id INTEGER NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    expires_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS steam_states (
    state      TEXT PRIMARY KEY,
    purpose    TEXT NOT NULL,
    account_id INTEGER,
    return_to  TEXT NOT NULL,
    expires_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS steam_nonces (
    nonce   TEXT PRIMARY KEY,
    seen_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS match_results (
    server     TEXT NOT NULL,
    match_id   TEXT NOT NULL,
    map        TEXT NOT NULL,
    mode       TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (server, match_id)
);
";

pub fn open(path: &str) -> Result<Connection, rusqlite::Error> {
    let c = if path == ":memory:" { Connection::open_in_memory()? } else { Connection::open(path)? };
    c.busy_timeout(std::time::Duration::from_secs(5))?;
    c.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")?;
    c.execute_batch(SCHEMA)?;
    c.execute_batch(crate::site::SCHEMA)?;
    Ok(c)
}

#[derive(Clone, Debug)]
pub struct Account {
    pub id: i64,
    pub username: String,
    pub password_hash: Option<String>,
    pub steam_id: Option<String>,
    pub callsign: String,
    pub emblem: String,
    pub classes: String,
    pub settings: String,
    pub banned: bool,
    pub kills: i64,
    pub deaths: i64,
    pub score: i64,
    pub wins: i64,
    pub matches: i64,
    pub created_at: i64,
}

const COLS: &str = "id, username, password_hash, steam_id, callsign, emblem, classes, settings, banned, kills, deaths, score, wins, matches, created_at";

fn account_from(r: &Row<'_>) -> rusqlite::Result<Account> {
    Ok(Account {
        id: r.get(0)?,
        username: r.get(1)?,
        password_hash: r.get(2)?,
        steam_id: r.get(3)?,
        callsign: r.get(4)?,
        emblem: r.get(5)?,
        classes: r.get(6)?,
        settings: r.get(7)?,
        banned: r.get::<_, i64>(8)? != 0,
        kills: r.get(9)?,
        deaths: r.get(10)?,
        score: r.get(11)?,
        wins: r.get(12)?,
        matches: r.get(13)?,
        created_at: r.get(14)?,
    })
}

pub fn create_account(
    c: &Connection,
    username: &str,
    password_hash: Option<&str>,
    steam_id: Option<&str>,
    callsign: &str,
    now: i64,
) -> Result<i64, DbErr> {
    c.execute(
        "INSERT INTO accounts (username, password_hash, steam_id, callsign, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![username, password_hash, steam_id, callsign, now],
    )?;
    Ok(c.last_insert_rowid())
}

pub fn account_by_id(c: &Connection, id: i64) -> Result<Option<Account>, DbErr> {
    Ok(c.query_row(&format!("SELECT {COLS} FROM accounts WHERE id = ?1"), [id], account_from).optional()?)
}

pub fn account_by_username(c: &Connection, username: &str) -> Result<Option<Account>, DbErr> {
    Ok(c.query_row(&format!("SELECT {COLS} FROM accounts WHERE username = ?1"), [username], account_from).optional()?)
}

pub fn account_by_steam(c: &Connection, steam_id: &str) -> Result<Option<Account>, DbErr> {
    Ok(c.query_row(&format!("SELECT {COLS} FROM accounts WHERE steam_id = ?1"), [steam_id], account_from).optional()?)
}

pub fn set_steam(c: &Connection, id: i64, steam_id: &str) -> Result<(), DbErr> {
    c.execute("UPDATE accounts SET steam_id = ?1 WHERE id = ?2", params![steam_id, id])?;
    Ok(())
}

pub fn clear_steam(c: &Connection, id: i64) -> Result<(), DbErr> {
    c.execute("UPDATE accounts SET steam_id = NULL WHERE id = ?1", [id])?;
    Ok(())
}

pub fn set_password(c: &Connection, id: i64, hash: &str) -> Result<(), DbErr> {
    c.execute("UPDATE accounts SET password_hash = ?1 WHERE id = ?2", params![hash, id])?;
    Ok(())
}

pub fn update_profile(
    c: &Connection,
    id: i64,
    callsign: Option<&str>,
    emblem: Option<&str>,
    classes: Option<&str>,
    settings: Option<&str>,
) -> Result<(), DbErr> {
    c.execute(
        "UPDATE accounts SET callsign = COALESCE(?1, callsign), emblem = COALESCE(?2, emblem), \
         classes = COALESCE(?3, classes), settings = COALESCE(?4, settings) WHERE id = ?5",
        params![callsign, emblem, classes, settings, id],
    )?;
    Ok(())
}

// ---- sessions, keys, tickets --------------------------------------------------

/// Add a session, keeping at most 20 per account (the oldest are dropped).
pub fn insert_session(c: &Connection, token_hash: &str, account_id: i64, now: i64, expires_at: i64) -> Result<(), DbErr> {
    c.execute(
        "INSERT INTO sessions (token_hash, account_id, created_at, expires_at) VALUES (?1, ?2, ?3, ?4)",
        params![token_hash, account_id, now, expires_at],
    )?;
    c.execute(
        "DELETE FROM sessions WHERE account_id = ?1 AND token_hash NOT IN \
         (SELECT token_hash FROM sessions WHERE account_id = ?1 ORDER BY created_at DESC, rowid DESC LIMIT 20)",
        [account_id],
    )?;
    Ok(())
}

pub fn session_account(c: &Connection, token_hash: &str, now: i64) -> Result<Option<Account>, DbErr> {
    let cols = COLS.split(", ").map(|n| format!("a.{n}")).collect::<Vec<_>>().join(", ");
    Ok(c.query_row(
        &format!(
            "SELECT {cols} FROM sessions s JOIN accounts a ON a.id = s.account_id \
             WHERE s.token_hash = ?1 AND s.expires_at > ?2"
        ),
        params![token_hash, now],
        account_from,
    )
    .optional()?)
}

pub fn delete_session(c: &Connection, token_hash: &str) -> Result<(), DbErr> {
    c.execute("DELETE FROM sessions WHERE token_hash = ?1", [token_hash])?;
    Ok(())
}

/// Sign out everywhere except `keep` (used after a password change).
pub fn delete_other_sessions(c: &Connection, account_id: i64, keep: &str) -> Result<(), DbErr> {
    c.execute("DELETE FROM sessions WHERE account_id = ?1 AND token_hash <> ?2", params![account_id, keep])?;
    Ok(())
}

/// Create or replace the account's launcher key.
pub fn put_key(c: &Connection, account_id: i64, key_hash: &str, now: i64) -> Result<(), DbErr> {
    c.execute(
        "INSERT INTO api_keys (account_id, key_hash, created_at, last_used) VALUES (?1, ?2, ?3, NULL) \
         ON CONFLICT(account_id) DO UPDATE SET key_hash = excluded.key_hash, created_at = excluded.created_at, last_used = NULL",
        params![account_id, key_hash, now],
    )?;
    Ok(())
}

pub fn key_account(c: &Connection, key_hash: &str, now: i64) -> Result<Option<Account>, DbErr> {
    let cols = COLS.split(", ").map(|n| format!("a.{n}")).collect::<Vec<_>>().join(", ");
    let acc = c
        .query_row(
            &format!("SELECT {cols} FROM api_keys k JOIN accounts a ON a.id = k.account_id WHERE k.key_hash = ?1"),
            [key_hash],
            account_from,
        )
        .optional()?;
    if acc.is_some() {
        c.execute(
            "UPDATE api_keys SET last_used = ?2 WHERE key_hash = ?1 AND (last_used IS NULL OR last_used < ?2 - 60)",
            params![key_hash, now],
        )?;
    }
    Ok(acc)
}

pub fn key_info(c: &Connection, account_id: i64) -> Result<Option<(i64, Option<i64>)>, DbErr> {
    Ok(c.query_row("SELECT created_at, last_used FROM api_keys WHERE account_id = ?1", [account_id], |r| {
        Ok((r.get(0)?, r.get(1)?))
    })
    .optional()?)
}

pub fn delete_key(c: &Connection, account_id: i64) -> Result<bool, DbErr> {
    Ok(c.execute("DELETE FROM api_keys WHERE account_id = ?1", [account_id])? > 0)
}

pub fn insert_ticket(c: &Connection, token_hash: &str, account_id: i64, expires_at: i64) -> Result<(), DbErr> {
    c.execute(
        "INSERT INTO tickets (token_hash, account_id, expires_at) VALUES (?1, ?2, ?3)",
        params![token_hash, account_id, expires_at],
    )?;
    Ok(())
}

/// Use a join ticket: it is deleted, so it works once.
pub fn take_ticket(c: &Connection, token_hash: &str, now: i64) -> Result<Option<Account>, DbErr> {
    let id: Option<i64> = c
        .query_row(
            "DELETE FROM tickets WHERE token_hash = ?1 AND expires_at > ?2 RETURNING account_id",
            params![token_hash, now],
            |r| r.get(0),
        )
        .optional()?;
    match id {
        Some(id) => account_by_id(c, id),
        None => Ok(None),
    }
}

pub fn insert_code(c: &Connection, code_hash: &str, account_id: i64, expires_at: i64) -> Result<(), DbErr> {
    c.execute(
        "INSERT INTO exchange_codes (code_hash, account_id, expires_at) VALUES (?1, ?2, ?3)",
        params![code_hash, account_id, expires_at],
    )?;
    Ok(())
}

pub fn take_code(c: &Connection, code_hash: &str, now: i64) -> Result<Option<Account>, DbErr> {
    let id: Option<i64> = c
        .query_row(
            "DELETE FROM exchange_codes WHERE code_hash = ?1 AND expires_at > ?2 RETURNING account_id",
            params![code_hash, now],
            |r| r.get(0),
        )
        .optional()?;
    match id {
        Some(id) => account_by_id(c, id),
        None => Ok(None),
    }
}

// ---- Steam sign-in state ------------------------------------------------------

pub struct SteamState {
    pub purpose: String,
    pub account_id: Option<i64>,
    pub return_to: String,
}

pub fn insert_state(
    c: &Connection,
    state: &str,
    purpose: &str,
    account_id: Option<i64>,
    return_to: &str,
    expires_at: i64,
) -> Result<(), DbErr> {
    c.execute(
        "INSERT INTO steam_states (state, purpose, account_id, return_to, expires_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![state, purpose, account_id, return_to, expires_at],
    )?;
    Ok(())
}

/// Use a sign-in state: it is deleted, so a callback works once.
pub fn take_state(c: &Connection, state: &str, now: i64) -> Result<Option<SteamState>, DbErr> {
    Ok(c.query_row(
        "DELETE FROM steam_states WHERE state = ?1 AND expires_at > ?2 RETURNING purpose, account_id, return_to",
        params![state, now],
        |r| Ok(SteamState { purpose: r.get(0)?, account_id: r.get(1)?, return_to: r.get(2)? }),
    )
    .optional()?)
}

/// Remember a Steam response nonce. False if it was seen before (a replay).
pub fn claim_nonce(c: &Connection, nonce: &str, now: i64) -> Result<bool, DbErr> {
    Ok(c.execute("INSERT OR IGNORE INTO steam_nonces (nonce, seen_at) VALUES (?1, ?2)", params![nonce, now])? == 1)
}

// ---- stats ----------------------------------------------------------------------

pub struct PlayerResult {
    pub account_id: i64,
    pub kills: i64,
    pub deaths: i64,
    pub score: i64,
    pub won: bool,
}

/// Record a finished match once. Returns false if this (server, match) was
/// already recorded, so a server can safely retry.
#[allow(clippy::too_many_arguments)]
pub fn record_match(
    c: &mut Connection,
    server: &str,
    match_id: &str,
    map: &str,
    mode: &str,
    region: &str,
    duration: i64,
    players: &[PlayerResult],
    now: i64,
) -> Result<bool, DbErr> {
    let tx = c.transaction()?;
    let fresh = tx.execute(
        "INSERT OR IGNORE INTO match_results (server, match_id, map, mode, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![server, match_id, map, mode, now],
    )? == 1;
    if fresh {
        for p in players {
            tx.execute(
                "UPDATE accounts SET kills = kills + ?1, deaths = deaths + ?2, score = score + ?3, \
                 wins = wins + ?4, matches = matches + 1 WHERE id = ?5",
                params![p.kills, p.deaths, p.score, i64::from(p.won), p.account_id],
            )?;
            tx.execute(
                "INSERT OR IGNORE INTO match_players (server, match_id, account_id, kills, deaths, score, won, mode, region, duration, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![server, match_id, p.account_id, p.kills, p.deaths, p.score, i64::from(p.won), mode, region, duration, now],
            )?;
        }
    }
    tx.commit()?;
    Ok(fresh)
}

/// Delete everything that has expired.
pub fn cleanup(c: &Connection, now: i64) -> Result<(), DbErr> {
    c.execute("DELETE FROM sessions WHERE expires_at <= ?1", [now])?;
    c.execute("DELETE FROM tickets WHERE expires_at <= ?1", [now])?;
    c.execute("DELETE FROM exchange_codes WHERE expires_at <= ?1", [now])?;
    c.execute("DELETE FROM steam_states WHERE expires_at <= ?1", [now])?;
    c.execute("DELETE FROM steam_nonces WHERE seen_at <= ?1", [now - 86_400])?;
    Ok(())
}
