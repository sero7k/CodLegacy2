//! The launcher's client against the real account service on a loopback port.

use codlegacy_accounts::{AppState, Config, SteamHttp, router};
use codlegacy_launcher::Fault;
use codlegacy_launcher::api::Client;
use codlegacy_launcher::settings::Settings;
use std::sync::Arc;

fn serve() -> String {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let state = AppState::new(Config::for_tests(), Arc::new(SteamHttp)).unwrap();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            tx.send(listener.local_addr().unwrap().port()).unwrap();
            axum::serve(listener, router(state)).await.unwrap();
        });
    });
    format!("http://127.0.0.1:{}", rx.recv().unwrap())
}

fn call(method: &str, url: &str, token: Option<&str>, body: Option<serde_json::Value>) -> serde_json::Value {
    let agent: ureq::Agent = ureq::Agent::config_builder().proxy(None).http_status_as_error(false).build().into();
    let mut req = ureq::http::Request::builder().method(method).uri(url);
    if let Some(t) = token {
        req = req.header("Authorization", format!("Bearer {t}"));
    }
    let req = match body {
        Some(b) => req.header("Content-Type", "application/json").body(b.to_string()).unwrap(),
        None => req.body(String::new()).unwrap(),
    };
    let mut resp = agent.run(req).unwrap();
    let text = resp.body_mut().read_to_string().unwrap();
    serde_json::from_str(&text).unwrap_or_else(|_| panic!("status {} body {text:?}", resp.status()))
}

#[test]
fn a_launcher_key_gets_a_ticket_and_a_bad_key_is_refused() {
    let base = serve();
    let reg = call("POST", &format!("{base}/register"), None, Some(serde_json::json!({"username": "alpha", "password": "password-123"})));
    let session = reg["token"].as_str().unwrap_or_else(|| panic!("no token in {reg}")).to_string();
    let key = call("POST", &format!("{base}/api-key"), Some(&session), None)["key"].as_str().unwrap().to_string();

    let good = Settings { api_url: base.clone(), key: key.clone(), ..Settings::default() };
    assert!(good.check_key().is_ok());
    let client = Client::new(&good).unwrap();
    assert_eq!(client.me().unwrap().callsign, "alpha");
    let t1 = client.join_ticket().unwrap();
    let t2 = client.join_ticket().unwrap();
    assert_ne!(t1.ticket, t2.ticket);
    assert_eq!(t1.expires_in, 60);
    assert!(client.servers().unwrap().is_empty());

    // A made-up key is refused with the support code for a cancelled key.
    let bad = Settings { api_url: base.clone(), key: "0".repeat(32), ..Settings::default() };
    assert_eq!(Client::new(&bad).unwrap().me().unwrap_err().fault, Fault::KeyCancelled);

    // Making a newer key cancels the old one.
    let _ = call("POST", &format!("{base}/api-key"), Some(&session), None);
    assert_eq!(client.join_ticket().unwrap_err().fault, Fault::KeyCancelled);

    // A service that is not there reads as "could not reach".
    let gone = Settings { api_url: "http://127.0.0.1:1".into(), key, ..Settings::default() };
    assert_eq!(Client::new(&gone).unwrap().me().unwrap_err().fault, Fault::Unreachable);
}
