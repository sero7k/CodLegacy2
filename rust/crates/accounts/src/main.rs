use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use codlegacy_accounts::{AppState, Config, SteamHttp, router};

#[tokio::main]
async fn main() {
    let cfg = match Config::from_env() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("accounts: configuration error: {e}");
            std::process::exit(2);
        }
    };
    let bind = cfg.bind.clone();
    println!("accounts: public url {}", cfg.public_url);
    println!("accounts: database {}", cfg.db_path);
    if let Some(dir) = &cfg.static_dir {
        println!("accounts: serving the site from {dir}");
    }
    if cfg.server_keys.is_empty() {
        println!("accounts: no ACCOUNTS_SERVER_KEYS set, so /verify and /results refuse every server");
    }
    let state = match AppState::new(cfg, Arc::new(SteamHttp)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("accounts: cannot open the database: {e}");
            std::process::exit(2);
        }
    };
    let sweep = state.clone();
    tokio::spawn(async move {
        loop {
            sweep.cleanup().await;
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
    });
    let listener = match tokio::net::TcpListener::bind(&bind).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("accounts: cannot listen on {bind}: {e}");
            std::process::exit(2);
        }
    };
    println!("accounts: listening on {bind}");
    let app = router(state).into_make_service_with_connect_info::<SocketAddr>();
    let _ = axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await;
}
