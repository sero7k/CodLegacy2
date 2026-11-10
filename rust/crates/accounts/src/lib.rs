//! CodLegacy account service.
//!
//! Players sign in on the website with Steam (or a username and password). The
//! website then issues a 32-character launcher key; the launcher stores it and
//! uses it to ask for short-lived, single-use join tickets that a game server
//! verifies before letting the player in.

pub mod config;
pub mod crypto;
pub mod db;
pub mod error;
pub mod limit;
mod routes;
mod site;
pub mod steam;

use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

pub use config::Config;
pub use error::{ApiError, DbErr};
pub use routes::router;
pub use steam::{SteamHttp, SteamVerifier};

#[derive(Clone)]
pub struct AppState(Arc<Inner>);

pub struct Inner {
    pub cfg: Config,
    conn: Arc<Mutex<rusqlite::Connection>>,
    pub limiter: limit::Limiter,
    pub hasher: Arc<crypto::Hasher>,
    pub steam: Arc<dyn SteamVerifier>,
    skew: AtomicI64,
}

impl std::ops::Deref for AppState {
    type Target = Inner;
    fn deref(&self) -> &Inner {
        &self.0
    }
}

impl AppState {
    pub fn new(cfg: Config, steam: Arc<dyn SteamVerifier>) -> Result<Self, rusqlite::Error> {
        let conn = db::open(&cfg.db_path)?;
        let hasher = Arc::new(crypto::Hasher::new(cfg.cheap_hashing));
        Ok(Self(Arc::new(Inner {
            cfg,
            conn: Arc::new(Mutex::new(conn)),
            limiter: limit::Limiter::default(),
            hasher,
            steam,
            skew: AtomicI64::new(0),
        })))
    }

    /// Seconds since the Unix epoch.
    pub fn now(&self) -> i64 {
        let real = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        real + self.skew.load(Ordering::Relaxed)
    }

    /// Move the service clock forward (tests of expiry).
    pub fn advance_clock(&self, secs: i64) {
        self.skew.fetch_add(secs, Ordering::Relaxed);
    }

    /// Run a database function on a blocking thread.
    pub async fn db<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut rusqlite::Connection) -> Result<T, DbErr> + Send + 'static,
    ) -> Result<T, DbErr> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let mut g = conn.lock().unwrap_or_else(|e| e.into_inner());
            f(&mut g)
        })
        .await
        .map_err(|_| DbErr::Other)?
    }

    /// Delete expired rows. Called on a timer by the binary.
    pub async fn cleanup(&self) {
        let now = self.now();
        let _ = self.db(move |c| db::cleanup(c, now)).await;
    }
}
