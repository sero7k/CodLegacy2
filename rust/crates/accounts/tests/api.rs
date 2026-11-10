//! End-to-end tests of the account service, with a fake standing in for Steam.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode, header};
use codlegacy_accounts::{AppState, Config, SteamVerifier, db, router, steam::OPENID_ENDPOINT};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const SITE: &str = "https://site.example.test";
const API: &str = "https://api.example.test";
const SERVER_KEY: &str = "test-server-key-0123456789";

struct FakeSteam {
    valid: AtomicBool,
    forms: Mutex<Vec<Vec<(String, String)>>>,
}

impl SteamVerifier for FakeSteam {
    fn check_authentication(&self, form: &[(String, String)]) -> Result<bool, String> {
        self.forms.lock().unwrap().push(form.to_vec());
        Ok(self.valid.load(Ordering::SeqCst))
    }
}

struct App {
    router: Router,
    state: AppState,
    steam: Arc<FakeSteam>,
}

fn app() -> App {
    let steam = Arc::new(FakeSteam { valid: AtomicBool::new(true), forms: Mutex::new(vec![]) });
    let state = AppState::new(Config::for_tests(), steam.clone()).unwrap();
    App { router: router(state.clone()), state, steam }
}

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: Value,
}

async fn send(app: &App, method: &str, uri: &str, token: Option<&str>, body: Option<Value>, extra: &[(&str, &str)]) -> Reply {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(t) = token {
        req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
    }
    for (k, v) in extra {
        req = req.header(*k, *v);
    }
    let req = match body {
        Some(b) => req.header(header::CONTENT_TYPE, "application/json").body(Body::from(b.to_string())).unwrap(),
        None => req.body(Body::empty()).unwrap(),
    };
    let resp = app.router.clone().oneshot(req).await.unwrap();
    let status = resp.status();
    let headers = resp.headers().clone();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    Reply { status, headers, body }
}

async fn register(app: &App, name: &str) -> String {
    let r = send(app, "POST", "/register", None, Some(json!({"username": name, "password": "password-123"})), &[]).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    r.body["token"].as_str().unwrap().to_string()
}

fn location(r: &Reply) -> url::Url {
    assert_eq!(r.status, StatusCode::SEE_OTHER, "{:?}", r.body);
    url::Url::parse(r.headers.get(header::LOCATION).unwrap().to_str().unwrap()).unwrap()
}

fn param(u: &url::Url, k: &str) -> Option<String> {
    u.query_pairs().find(|(n, _)| n == k).map(|(_, v)| v.into_owned())
}

fn enc(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// What Steam sends the browser back with.
fn steam_query(state: &str, steam_id: &str, nonce: &str, return_to: &str) -> String {
    let claimed = format!("https://steamcommunity.com/openid/id/{steam_id}");
    let mut s = url::form_urlencoded::Serializer::new(String::new());
    s.append_pair("state", state);
    for (k, v) in [
        ("ns", "http://specs.openid.net/auth/2.0"),
        ("mode", "id_res"),
        ("op_endpoint", OPENID_ENDPOINT),
        ("claimed_id", claimed.as_str()),
        ("identity", claimed.as_str()),
        ("return_to", return_to),
        ("response_nonce", nonce),
        ("assoc_handle", "1234567890"),
        ("signed", "signed,op_endpoint,claimed_id,identity,return_to,response_nonce,assoc_handle"),
        ("sig", "c2ln"),
    ] {
        s.append_pair(&format!("openid.{k}"), v);
    }
    s.finish()
}

/// Start a Steam flow and return (state, the return_to Steam was given).
async fn steam_start(app: &App, ret: &str) -> (String, String) {
    let r = send(app, "GET", &format!("/auth/steam/start?return={}", enc(ret)), None, None, &[]).await;
    let loc = location(&r);
    assert!(loc.as_str().starts_with(OPENID_ENDPOINT));
    let return_to = param(&loc, "openid.return_to").unwrap();
    let state = param(&url::Url::parse(&return_to).unwrap(), "state").unwrap();
    (state, return_to)
}

/// Run a full Steam sign-in; returns the redirect back to the page.
async fn steam_login(app: &App, steam_id: &str, nonce: &str) -> url::Url {
    let (state, return_to) = steam_start(app, &format!("{SITE}/account")).await;
    let q = steam_query(&state, steam_id, nonce, &return_to);
    location(&send(app, "GET", &format!("/auth/steam/callback?{q}"), None, None, &[]).await)
}

async fn exchange(app: &App, code: &str) -> Reply {
    send(app, "POST", "/auth/exchange", None, Some(json!({"code": code})), &[]).await
}

// ---- password accounts -----------------------------------------------------------

#[tokio::test]
async fn register_login_me_logout() {
    let app = app();
    let token = register(&app, "Sero7k").await;
    let me = send(&app, "GET", "/me", Some(&token), None, &[]).await;
    assert_eq!(me.status, StatusCode::OK);
    assert_eq!(me.body["username"], "Sero7k");
    assert_eq!(me.body["has_password"], true);
    assert!(me.body["steam_id"].is_null());

    // Usernames are unique regardless of case.
    let dup = send(&app, "POST", "/register", None, Some(json!({"username": "sero7K", "password": "password-123"})), &[]).await;
    assert_eq!(dup.status, StatusCode::CONFLICT);

    let login = send(&app, "POST", "/login", None, Some(json!({"username": "sero7k", "password": "password-123"})), &[]).await;
    assert_eq!(login.status, StatusCode::OK);
    let token2 = login.body["token"].as_str().unwrap().to_string();
    assert_ne!(token, token2);

    assert_eq!(send(&app, "POST", "/logout", Some(&token2), None, &[]).await.status, StatusCode::NO_CONTENT);
    assert_eq!(send(&app, "GET", "/me", Some(&token2), None, &[]).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(send(&app, "GET", "/me", Some(&token), None, &[]).await.status, StatusCode::OK);
    assert_eq!(send(&app, "GET", "/me", None, None, &[]).await.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn registration_rules() {
    let app = app();
    for (name, pw) in [("ab", "password-123"), ("has space", "password-123"), ("steam_123", "password-123"), ("okname", "short")] {
        let r = send(&app, "POST", "/register", None, Some(json!({"username": name, "password": pw})), &[]).await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{name}");
    }
}

#[tokio::test]
async fn wrong_passwords_are_throttled() {
    let app = app();
    register(&app, "victim").await;
    for _ in 0..5 {
        let r = send(&app, "POST", "/login", None, Some(json!({"username": "victim", "password": "nope-nope"})), &[]).await;
        assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    }
    // Locked out, even with the right password, until the failures age out.
    let r = send(&app, "POST", "/login", None, Some(json!({"username": "victim", "password": "password-123"})), &[]).await;
    assert_eq!(r.status, StatusCode::TOO_MANY_REQUESTS);
    app.state.advance_clock(301);
    let r = send(&app, "POST", "/login", None, Some(json!({"username": "victim", "password": "password-123"})), &[]).await;
    assert_eq!(r.status, StatusCode::OK);
}

#[tokio::test]
async fn unknown_user_looks_like_wrong_password() {
    let app = app();
    let r = send(&app, "POST", "/login", None, Some(json!({"username": "nobody", "password": "password-123"})), &[]).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    assert_eq!(r.body["error"], "invalid_credentials");
}

#[tokio::test]
async fn sessions_expire() {
    let app = app();
    let token = register(&app, "shortlived").await;
    app.state.advance_clock(31 * 86_400);
    assert_eq!(send(&app, "GET", "/me", Some(&token), None, &[]).await.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn profile_update_and_validation() {
    let app = app();
    let token = register(&app, "profiler").await;
    let ok = send(
        &app,
        "PUT",
        "/profile",
        Some(&token),
        Some(json!({"callsign": "Ghost 6", "emblem": "skull", "classes": [{"name": "AR"}], "settings": {"sens": 5}})),
        &[],
    )
    .await;
    assert_eq!(ok.status, StatusCode::OK, "{:?}", ok.body);
    let p = send(&app, "GET", "/profile", Some(&token), None, &[]).await;
    assert_eq!(p.body["account"]["callsign"], "Ghost 6");
    assert_eq!(p.body["classes"][0]["name"], "AR");
    assert_eq!(p.body["settings"]["sens"], 5);

    for bad in [
        json!({"callsign": "x"}),
        json!({"callsign": "<script>"}),
        json!({"classes": "not a list"}),
        json!({"settings": [1]}),
        json!({"unknown_field": 1}),
        json!({"classes": ["x".repeat(40_000)]}),
    ] {
        let r = send(&app, "PUT", "/profile", Some(&token), Some(bad.clone()), &[]).await;
        assert!(r.status.is_client_error(), "{bad}");
    }
    // A failed update changed nothing.
    let p = send(&app, "GET", "/profile", Some(&token), None, &[]).await;
    assert_eq!(p.body["account"]["callsign"], "Ghost 6");
}

#[tokio::test]
async fn banned_accounts_are_locked_out() {
    let app = app();
    let token = register(&app, "cheater").await;
    app.state.db(|c| Ok(c.execute("UPDATE accounts SET banned = 1", [])?)).await.unwrap();
    assert_eq!(send(&app, "GET", "/me", Some(&token), None, &[]).await.status, StatusCode::FORBIDDEN);
    let r = send(&app, "POST", "/login", None, Some(json!({"username": "cheater", "password": "password-123"})), &[]).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);
}

// ---- Steam ----------------------------------------------------------------------------

#[tokio::test]
async fn steam_sign_in_creates_then_reuses_an_account() {
    let app = app();
    let back = steam_login(&app, "76561198000000001", "n1").await;
    assert_eq!(format!("{}://{}{}", back.scheme(), back.host_str().unwrap(), back.path()), format!("{SITE}/account"));
    let code = param(&back, "code").expect("a code");

    // Steam was asked to confirm its signature, in check_authentication mode.
    {
        let forms = app.steam.forms.lock().unwrap();
        assert_eq!(forms.len(), 1);
        assert!(forms[0].contains(&("openid.mode".into(), "check_authentication".into())));
        assert!(forms[0].iter().any(|(k, _)| k == "openid.sig"));
    }

    let first = exchange(&app, &code).await;
    assert_eq!(first.status, StatusCode::OK, "{:?}", first.body);
    assert_eq!(first.body["account"]["steam_id"], "76561198000000001");
    assert_eq!(first.body["account"]["username"], "steam_76561198000000001");
    assert_eq!(first.body["account"]["has_password"], false);
    let id = first.body["account"]["id"].clone();

    // The code works once.
    assert_eq!(exchange(&app, &code).await.status, StatusCode::UNAUTHORIZED);

    // Signing in again with the same Steam account is the same player.
    let back = steam_login(&app, "76561198000000001", "n2").await;
    let second = exchange(&app, &param(&back, "code").unwrap()).await;
    assert_eq!(second.body["account"]["id"], id);
}

#[tokio::test]
async fn steam_refusals_send_the_browser_back_with_an_error() {
    let app = app();

    // Steam says the signature is not valid.
    app.steam.valid.store(false, Ordering::SeqCst);
    let back = steam_login(&app, "76561198000000001", "n1").await;
    assert_eq!(param(&back, "error").as_deref(), Some("steam_rejected"));
    assert!(param(&back, "code").is_none());
    app.steam.valid.store(true, Ordering::SeqCst);

    // A response nonce can be used once.
    let ok = steam_login(&app, "76561198000000001", "same-nonce").await;
    assert!(param(&ok, "code").is_some());
    let replay = steam_login(&app, "76561198000000001", "same-nonce").await;
    assert_eq!(param(&replay, "error").as_deref(), Some("steam_replay"));

    // An assertion made for a different sign-in does not work on this one.
    let (state, _) = steam_start(&app, &format!("{SITE}/account")).await;
    let other_return = format!("{API}/auth/steam/callback?state=someone-elses");
    let q = steam_query(&state, "76561198000000001", "n3", &other_return);
    let back = location(&send(&app, "GET", &format!("/auth/steam/callback?{q}"), None, None, &[]).await);
    assert_eq!(param(&back, "error").as_deref(), Some("steam_invalid"));

    // Nothing was created by any of the refusals except the one good sign-in.
    let n: i64 = app.state.db(|c| Ok(c.query_row("SELECT COUNT(*) FROM accounts", [], |r| r.get(0))?)).await.unwrap();
    assert_eq!(n, 1);
}

#[tokio::test]
async fn steam_state_is_single_use_and_must_exist() {
    let app = app();
    let (state, return_to) = steam_start(&app, &format!("{SITE}/account")).await;
    let q = steam_query(&state, "76561198000000001", "n1", &return_to);
    let first = send(&app, "GET", &format!("/auth/steam/callback?{q}"), None, None, &[]).await;
    assert_eq!(first.status, StatusCode::SEE_OTHER);
    let again = send(&app, "GET", &format!("/auth/steam/callback?{q}"), None, None, &[]).await;
    assert_eq!(again.status, StatusCode::BAD_REQUEST);

    let q = steam_query("never-issued", "76561198000000001", "n2", &return_to);
    assert_eq!(send(&app, "GET", &format!("/auth/steam/callback?{q}"), None, None, &[]).await.status, StatusCode::BAD_REQUEST);

    // And an old state expires.
    let (state, return_to) = steam_start(&app, &format!("{SITE}/account")).await;
    app.state.advance_clock(601);
    let q = steam_query(&state, "76561198000000001", "n3", &return_to);
    assert_eq!(send(&app, "GET", &format!("/auth/steam/callback?{q}"), None, None, &[]).await.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn sign_in_only_returns_to_allowed_places() {
    let app = app();
    for ok in [format!("{SITE}/account"), "http://127.0.0.1:53211/cb".to_string(), "http://localhost:8123/".to_string()] {
        let r = send(&app, "GET", &format!("/auth/steam/start?return={}", enc(&ok)), None, None, &[]).await;
        assert_eq!(r.status, StatusCode::SEE_OTHER, "{ok}");
    }
    for bad in [
        "https://evil.test/".to_string(),
        "http://site.example.test/".to_string(),
        "https://user:pw@site.example.test/".to_string(),
        "https://site.example.test.evil.test/".to_string(),
        "http://evil.test:80/127.0.0.1".to_string(),
        "javascript:alert(1)".to_string(),
        "not a url".to_string(),
    ] {
        let r = send(&app, "GET", &format!("/auth/steam/start?return={}", enc(&bad)), None, None, &[]).await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{bad}");
    }
}

#[tokio::test]
async fn linking_and_unlinking_steam() {
    let app = app();
    let token = register(&app, "linker").await;

    let r = send(&app, "POST", "/auth/steam/link", Some(&token), Some(json!({"return": format!("{SITE}/account")})), &[]).await;
    assert_eq!(r.status, StatusCode::OK, "{:?}", r.body);
    let url = url::Url::parse(r.body["url"].as_str().unwrap()).unwrap();
    let return_to = param(&url, "openid.return_to").unwrap();
    let state = param(&url::Url::parse(&return_to).unwrap(), "state").unwrap();
    let q = steam_query(&state, "76561198000000009", "n1", &return_to);
    let back = location(&send(&app, "GET", &format!("/auth/steam/callback?{q}"), None, None, &[]).await);
    assert_eq!(param(&back, "linked").as_deref(), Some("1"));
    let me = send(&app, "GET", "/me", Some(&token), None, &[]).await;
    assert_eq!(me.body["steam_id"], "76561198000000009");

    // Signing in with that Steam account now reaches the same player.
    let back = steam_login(&app, "76561198000000009", "n2").await;
    let via_steam = exchange(&app, &param(&back, "code").unwrap()).await;
    assert_eq!(via_steam.body["account"]["username"], "linker");

    // A second account cannot claim the same Steam account.
    let other = register(&app, "latecomer").await;
    let r = send(&app, "POST", "/auth/steam/link", Some(&other), Some(json!({"return": format!("{SITE}/account")})), &[]).await;
    let url = url::Url::parse(r.body["url"].as_str().unwrap()).unwrap();
    let return_to = param(&url, "openid.return_to").unwrap();
    let state = param(&url::Url::parse(&return_to).unwrap(), "state").unwrap();
    let q = steam_query(&state, "76561198000000009", "n3", &return_to);
    let back = location(&send(&app, "GET", &format!("/auth/steam/callback?{q}"), None, None, &[]).await);
    assert_eq!(param(&back, "error").as_deref(), Some("steam_in_use"));

    // Unlinking works because there is a password to fall back on.
    assert_eq!(send(&app, "DELETE", "/auth/steam", Some(&token), None, &[]).await.status, StatusCode::NO_CONTENT);
    assert!(send(&app, "GET", "/me", Some(&token), None, &[]).await.body["steam_id"].is_null());
}

#[tokio::test]
async fn a_steam_only_account_must_set_a_password_before_unlinking() {
    let app = app();
    let back = steam_login(&app, "76561198000000002", "n1").await;
    let token = exchange(&app, &param(&back, "code").unwrap()).await.body["token"].as_str().unwrap().to_string();

    let r = send(&app, "DELETE", "/auth/steam", Some(&token), None, &[]).await;
    assert_eq!(r.status, StatusCode::CONFLICT);
    assert_eq!(r.body["error"], "set_password_first");

    // No current password is needed when none is set yet.
    let r = send(&app, "POST", "/password", Some(&token), Some(json!({"new": "a-new-password"})), &[]).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(send(&app, "DELETE", "/auth/steam", Some(&token), None, &[]).await.status, StatusCode::NO_CONTENT);

    // The username lets them in with the new password.
    let r = send(&app, "POST", "/login", None, Some(json!({"username": "steam_76561198000000002", "password": "a-new-password"})), &[]).await;
    assert_eq!(r.status, StatusCode::OK);
}

#[tokio::test]
async fn changing_a_password_needs_the_old_one_and_signs_out_other_sessions() {
    let app = app();
    let a = register(&app, "changer").await;
    let b = send(&app, "POST", "/login", None, Some(json!({"username": "changer", "password": "password-123"})), &[]).await.body["token"]
        .as_str()
        .unwrap()
        .to_string();
    let r = send(&app, "POST", "/password", Some(&a), Some(json!({"current": "wrong-wrong", "new": "brand-new-pass"})), &[]).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    let r = send(&app, "POST", "/password", Some(&a), Some(json!({"current": "password-123", "new": "brand-new-pass"})), &[]).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(send(&app, "GET", "/me", Some(&a), None, &[]).await.status, StatusCode::OK);
    assert_eq!(send(&app, "GET", "/me", Some(&b), None, &[]).await.status, StatusCode::UNAUTHORIZED);
}

// ---- launcher key and joining -------------------------------------------------------

#[tokio::test]
async fn the_site_issues_a_launcher_key_and_the_launcher_uses_it() {
    let app = app();
    let back = steam_login(&app, "76561198000000003", "n1").await;
    let web = exchange(&app, &param(&back, "code").unwrap()).await.body["token"].as_str().unwrap().to_string();

    assert_eq!(send(&app, "GET", "/api-key", Some(&web), None, &[]).await.body["exists"], false);
    let made = send(&app, "POST", "/api-key", Some(&web), None, &[]).await;
    assert_eq!(made.status, StatusCode::OK);
    let key = made.body["key"].as_str().unwrap().to_string();
    assert_eq!(key.len(), 32);
    assert!(key.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(send(&app, "GET", "/api-key", Some(&web), None, &[]).await.body["exists"], true);

    // The key is not stored in the clear.
    let stored: String = app.state.db(|c| Ok(c.query_row("SELECT key_hash FROM api_keys", [], |r| r.get(0))?)).await.unwrap();
    assert_ne!(stored, key);

    // The launcher can read the account and ask for a join ticket...
    let me = send(&app, "GET", "/me", Some(&key), None, &[]).await;
    assert_eq!(me.status, StatusCode::OK);
    assert_eq!(me.body["steam_id"], "76561198000000003");
    assert_eq!(send(&app, "GET", "/me", Some(&key.to_uppercase()), None, &[]).await.status, StatusCode::OK);
    assert_eq!(send(&app, "POST", "/join-ticket", Some(&key), None, &[]).await.status, StatusCode::OK);

    // ...but cannot manage keys, passwords or the Steam link.
    for (m, p) in [("POST", "/api-key"), ("GET", "/api-key"), ("DELETE", "/api-key"), ("DELETE", "/auth/steam"), ("POST", "/logout")] {
        let r = send(&app, m, p, Some(&key), None, &[]).await;
        assert_eq!(r.status, StatusCode::FORBIDDEN, "{m} {p}");
    }
    let r = send(&app, "POST", "/password", Some(&key), Some(json!({"new": "another-password"})), &[]).await;
    assert_eq!(r.status, StatusCode::FORBIDDEN);

    // Making a new key replaces the old one.
    let new_key = send(&app, "POST", "/api-key", Some(&web), None, &[]).await.body["key"].as_str().unwrap().to_string();
    assert_ne!(new_key, key);
    assert_eq!(send(&app, "GET", "/me", Some(&key), None, &[]).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(send(&app, "GET", "/me", Some(&new_key), None, &[]).await.status, StatusCode::OK);

    // Revoking stops it.
    assert_eq!(send(&app, "DELETE", "/api-key", Some(&web), None, &[]).await.status, StatusCode::NO_CONTENT);
    assert_eq!(send(&app, "GET", "/me", Some(&new_key), None, &[]).await.status, StatusCode::UNAUTHORIZED);
    // A made-up key is just refused.
    assert_eq!(send(&app, "GET", "/me", Some(&"0".repeat(32)), None, &[]).await.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn join_tickets_work_once_for_a_game_server() {
    let app = app();
    let web = register(&app, "joiner").await;
    let key = send(&app, "POST", "/api-key", Some(&web), None, &[]).await.body["key"].as_str().unwrap().to_string();
    let ticket = send(&app, "POST", "/join-ticket", Some(&key), None, &[]).await.body["ticket"].as_str().unwrap().to_string();

    // Not a server: refused. A wrong key: refused.
    let body = || Some(json!({"ticket": ticket}));
    assert_eq!(send(&app, "POST", "/verify", None, body(), &[]).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(send(&app, "POST", "/verify", None, body(), &[("x-server-key", "wrong-wrong-wrong-wrong-wrong")]).await.status, StatusCode::UNAUTHORIZED);
    // A player's own key is not a server key.
    assert_eq!(send(&app, "POST", "/verify", Some(&key), body(), &[]).await.status, StatusCode::UNAUTHORIZED);

    let ok = send(&app, "POST", "/verify", None, body(), &[("x-server-key", SERVER_KEY)]).await;
    assert_eq!(ok.status, StatusCode::OK, "{:?}", ok.body);
    assert_eq!(ok.body["callsign"], "joiner");
    // The ticket worked once.
    assert_eq!(send(&app, "POST", "/verify", None, body(), &[("x-server-key", SERVER_KEY)]).await.status, StatusCode::NOT_FOUND);

    // An unused ticket expires after a minute.
    let ticket2 = send(&app, "POST", "/join-ticket", Some(&key), None, &[]).await.body["ticket"].as_str().unwrap().to_string();
    app.state.advance_clock(61);
    let r = send(&app, "POST", "/verify", None, Some(json!({"ticket": ticket2})), &[("x-server-key", SERVER_KEY)]).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn match_results_feed_the_leaderboard_once() {
    let app = app();
    let a = register(&app, "alpha").await;
    let b = register(&app, "bravo").await;
    let id_a = send(&app, "GET", "/me", Some(&a), None, &[]).await.body["id"].as_i64().unwrap();
    let id_b = send(&app, "GET", "/me", Some(&b), None, &[]).await.body["id"].as_i64().unwrap();

    assert_eq!(send(&app, "GET", "/leaderboard", None, None, &[]).await.body, json!([]));

    let report = json!({"match_id": "m-1", "map": "nuketown", "mode": "tdm", "players": [
        {"account_id": id_a, "kills": 20, "deaths": 5, "score": 2500, "won": true},
        {"account_id": id_b, "kills": 8, "deaths": 15, "score": 900, "won": false},
    ]});
    let hdr = [("x-server-key", SERVER_KEY)];
    assert_eq!(send(&app, "POST", "/results", None, Some(report.clone()), &[]).await.status, StatusCode::UNAUTHORIZED);
    let r = send(&app, "POST", "/results", None, Some(report.clone()), &hdr).await;
    assert_eq!(r.body["recorded"], true);
    // A server retrying the same match changes nothing.
    let r = send(&app, "POST", "/results", None, Some(report), &hdr).await;
    assert_eq!(r.body["recorded"], false);

    let board = send(&app, "GET", "/leaderboard?limit=10", None, None, &[]).await;
    assert_eq!(board.body[0]["callsign"], "alpha");
    assert_eq!(board.body[0]["score"], 2500);
    assert_eq!(board.body[0]["wins"], 1);
    assert_eq!(board.body[0]["matches"], 1);
    assert_eq!(board.body[1]["callsign"], "bravo");
    assert_eq!(board.body[1]["rank"], 2);

    // Absurd numbers are refused.
    let bad = json!({"match_id": "m-2", "map": "x", "mode": "y", "players": [
        {"account_id": id_a, "kills": 99999, "deaths": 0, "score": 0, "won": false}]});
    assert_eq!(send(&app, "POST", "/results", None, Some(bad), &hdr).await.status, StatusCode::BAD_REQUEST);
}

// ---- website features --------------------------------------------------------------

const ADMIN: [(&str, &str); 1] = [("x-admin-key", "test-admin-key-0123456789abcdef")];

#[tokio::test]
async fn leaderboard_sums_best_ten_games_and_filters() {
    let app = app();
    let a = register(&app, "alpha").await;
    let id_a = send(&app, "GET", "/me", Some(&a), None, &[]).await.body["id"].as_i64().unwrap();
    let hdr = [("x-server-key", SERVER_KEY)];
    for i in 0..12 {
        let region = if i == 0 { "NA" } else { "EU" };
        let r = json!({"match_id": format!("m{i}"), "map": "nuketown", "mode": "tdm", "region": region, "duration_secs": 600, "players": [
            {"account_id": id_a, "kills": 1, "deaths": 1, "score": 100 + i, "won": i % 2 == 0}]});
        assert_eq!(send(&app, "POST", "/results", None, Some(r), &hdr).await.body["recorded"], true);
    }
    let all = send(&app, "GET", "/leaderboard", None, None, &[]).await;
    // best ten of 100..=111 is 102..=111
    assert_eq!(all.body[0]["score"], (102..=111).sum::<i64>());
    assert_eq!(all.body[0]["games"], 12);
    assert_eq!(all.body[0]["best"], 111);
    let na = send(&app, "GET", "/leaderboard?region=NA", None, None, &[]).await;
    assert_eq!(na.body[0]["score"], 100);
    assert_eq!(send(&app, "GET", "/leaderboard?q=zzz", None, None, &[]).await.body, json!([]));
    assert_eq!(send(&app, "GET", "/leaderboard?q=alp", None, None, &[]).await.body[0]["callsign"], "alpha");
    assert_eq!(send(&app, "GET", "/leaderboard?region=XX", None, None, &[]).await.status, StatusCode::BAD_REQUEST);
    assert_eq!(send(&app, "GET", "/leaderboard?season=7d", None, None, &[]).await.body[0]["games"], 12);

    let st = send(&app, "GET", "/me/stats", Some(&a), None, &[]).await;
    assert_eq!(st.body["playtime_secs"], 7200);
    assert_eq!(st.body["matches"], 12);
}

#[tokio::test]
async fn servers_appear_while_they_heartbeat() {
    let app = app();
    let hb = json!({"region": "EU", "mode": "tdm", "map": "nuketown", "players": 5, "max_players": 12});
    assert_eq!(send(&app, "POST", "/servers/heartbeat", None, Some(hb.clone()), &[]).await.status, StatusCode::UNAUTHORIZED);
    let hdr = [("x-server-key", SERVER_KEY)];
    assert_eq!(send(&app, "POST", "/servers/heartbeat", None, Some(hb), &hdr).await.status, StatusCode::NO_CONTENT);
    let bad = json!({"region": "EU", "mode": "tdm", "map": "x", "players": 50, "max_players": 12});
    assert_eq!(send(&app, "POST", "/servers/heartbeat", None, Some(bad), &hdr).await.status, StatusCode::BAD_REQUEST);
    let list = send(&app, "GET", "/servers", None, None, &[]).await;
    assert_eq!(list.body["servers"][0]["name"], "test");
    assert_eq!(list.body["servers"][0]["players"], 5);
    // Old heartbeats age out.
    app.state.db(|c| Ok(c.execute("UPDATE live_servers SET seen_at = 0", [])?)).await.unwrap();
    assert_eq!(send(&app, "GET", "/servers", None, None, &[]).await.body["servers"], json!([]));
}

#[tokio::test]
async fn teams_have_limits_and_leaders() {
    let app = app();
    let a = register(&app, "alpha").await;
    let b = register(&app, "bravo").await;
    assert_eq!(send(&app, "GET", "/teams", None, None, &[]).await.status, StatusCode::UNAUTHORIZED);
    let r = send(&app, "POST", "/teams", Some(&a), Some(json!({"name": "Night Owls", "tag": "OWL"})), &[]).await;
    assert_eq!(r.status, StatusCode::CREATED);
    let id = r.body["id"].as_i64().unwrap();
    assert_eq!(send(&app, "POST", "/teams", Some(&b), Some(json!({"name": "night owls", "tag": "XYZ"})), &[]).await.status, StatusCode::CONFLICT);
    assert_eq!(send(&app, "POST", "/teams", Some(&b), Some(json!({"name": "Other", "tag": "o!"})), &[]).await.status, StatusCode::BAD_REQUEST);
    let list = send(&app, "GET", "/teams", Some(&a), None, &[]).await;
    assert_eq!(list.body["teams"][0]["role"], "leader");
    // Leaders cannot "leave", and only leaders delete.
    assert_eq!(send(&app, "POST", &format!("/teams/{id}/leave"), Some(&a), None, &[]).await.status, StatusCode::FORBIDDEN);
    assert_eq!(send(&app, "DELETE", &format!("/teams/{id}"), Some(&b), None, &[]).await.status, StatusCode::FORBIDDEN);
    assert_eq!(send(&app, "DELETE", &format!("/teams/{id}"), Some(&a), None, &[]).await.status, StatusCode::NO_CONTENT);
    // At most four led teams.
    for i in 0..4 {
        let r = send(&app, "POST", "/teams", Some(&a), Some(json!({"name": format!("Team {i}"), "tag": format!("T{i}")})), &[]).await;
        assert_eq!(r.status, StatusCode::CREATED);
    }
    assert_eq!(send(&app, "POST", "/teams", Some(&a), Some(json!({"name": "Team 5", "tag": "T5"})), &[]).await.status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn tickets_and_creator_requests_with_admin_replies() {
    let app = app();
    let a = register(&app, "alpha").await;
    let bad = json!({"category": "nope", "subject": "s", "body": "b"});
    assert_eq!(send(&app, "POST", "/tickets", Some(&a), Some(bad), &[]).await.status, StatusCode::BAD_REQUEST);
    let t = json!({"category": "launcher", "subject": "L3 error", "body": "line one\nline two"});
    let r = send(&app, "POST", "/tickets", Some(&a), Some(t.clone()), &[]).await;
    assert_eq!(r.status, StatusCode::CREATED);
    let id = r.body["id"].as_i64().unwrap();
    assert_eq!(send(&app, "GET", "/admin/tickets", None, None, &[]).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(send(&app, "GET", "/admin/tickets", None, None, &[("x-admin-key", "wrong")]).await.status, StatusCode::UNAUTHORIZED);
    let q = send(&app, "GET", "/admin/tickets?status=open", None, None, &ADMIN).await;
    assert_eq!(q.body["tickets"][0]["callsign"], "alpha");
    let reply = json!({"reply": "Fixed, try again.", "close": true});
    assert_eq!(send(&app, "POST", &format!("/admin/tickets/{id}"), None, Some(reply), &ADMIN).await.status, StatusCode::NO_CONTENT);
    let mine = send(&app, "GET", "/tickets", Some(&a), None, &[]).await;
    assert_eq!(mine.body["tickets"][0]["status"], "closed");
    assert_eq!(mine.body["tickets"][0]["reply"], "Fixed, try again.");
    let c = json!({"twitch": "twitch.tv/x", "kind": "event", "title": "Cup", "message": "hello"});
    assert_eq!(send(&app, "POST", "/creator-requests", Some(&a), Some(c), &[]).await.status, StatusCode::CREATED);
    assert_eq!(send(&app, "GET", "/admin/creator-requests", None, None, &ADMIN).await.body["requests"][0]["title"], "Cup");
}

#[tokio::test]
async fn overlay_token_shows_public_stats_and_can_rotate() {
    let app = app();
    let a = register(&app, "alpha").await;
    let o = send(&app, "GET", "/overlay", Some(&a), None, &[]).await;
    let token = o.body["token"].as_str().unwrap().to_string();
    assert_eq!(o.body["fields"].as_array().unwrap().len(), 4);
    let put = |f: Value| send(&app, "PUT", "/overlay", Some(&a), Some(json!({"fields": f})), &[]);
    assert_eq!(put(json!(["kd", "kd"])).await.status, StatusCode::BAD_REQUEST);
    assert_eq!(put(json!(["a", "b"])).await.status, StatusCode::BAD_REQUEST);
    assert_eq!(put(json!(["kd", "kills"])).await.status, StatusCode::NO_CONTENT);
    let public = send(&app, "GET", &format!("/overlay/{token}"), None, None, &[]).await;
    assert_eq!(public.body["callsign"], "alpha");
    assert_eq!(public.body["fields"], json!(["kd", "kills"]));
    let new = send(&app, "POST", "/overlay/regenerate", Some(&a), None, &[]).await.body["token"].as_str().unwrap().to_string();
    assert_ne!(new, token);
    assert_eq!(send(&app, "GET", &format!("/overlay/{token}"), None, None, &[]).await.status, StatusCode::NOT_FOUND);
    assert_eq!(send(&app, "GET", &format!("/overlay/{new}"), None, None, &[]).await.status, StatusCode::OK);
}

#[tokio::test]
async fn competitions_are_admin_created_and_public() {
    let app = app();
    let c = json!({"kind": "league", "title": "Spring League", "creator": "Admin", "mode": "tdm", "region": "EU", "status": "applications"});
    assert_eq!(send(&app, "POST", "/admin/competitions", None, Some(c.clone()), &[]).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(send(&app, "POST", "/admin/competitions", None, Some(c), &ADMIN).await.status, StatusCode::CREATED);
    assert_eq!(send(&app, "GET", "/competitions?kind=league", None, None, &[]).await.body["competitions"][0]["title"], "Spring League");
    assert_eq!(send(&app, "GET", "/competitions?kind=tournament", None, None, &[]).await.body["competitions"], json!([]));
}

#[tokio::test]
async fn admin_ban_locks_the_account_and_hides_it_from_the_board() {
    let app = app();
    let a = register(&app, "alpha").await;
    let id = send(&app, "GET", "/me", Some(&a), None, &[]).await.body["id"].as_i64().unwrap();
    assert_eq!(send(&app, "POST", &format!("/admin/accounts/{id}/ban"), None, Some(json!({"banned": true})), &ADMIN).await.status, StatusCode::NO_CONTENT);
    assert_eq!(send(&app, "GET", "/me", Some(&a), None, &[]).await.status, StatusCode::UNAUTHORIZED);
    assert_eq!(send(&app, "POST", "/admin/accounts/9999/ban", None, Some(json!({"banned": true})), &ADMIN).await.status, StatusCode::NOT_FOUND);
}

// ---- browser access --------------------------------------------------------------

#[tokio::test]
async fn only_known_sites_get_cors_access() {
    let app = app();
    let r = send(&app, "OPTIONS", "/me", None, None, &[("origin", SITE), ("access-control-request-method", "GET")]).await;
    assert_eq!(r.status, StatusCode::NO_CONTENT);
    assert_eq!(r.headers.get("access-control-allow-origin").unwrap(), SITE);
    assert!(r.headers.get("access-control-allow-headers").unwrap().to_str().unwrap().contains("authorization"));

    let r = send(&app, "GET", "/health", None, None, &[("origin", "https://evil.test")]).await;
    assert!(r.headers.get("access-control-allow-origin").is_none());
    assert_eq!(r.headers.get("x-content-type-options").unwrap(), "nosniff");
}

fn lookup(pairs: &[(&str, &str)]) -> Result<Config, String> {
    let m: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    Config::from_lookup(|k| m.get(k).cloned())
}

#[test]
fn configuration_rules() {
    // Defaults suit local testing.
    let c = lookup(&[]).unwrap();
    assert_eq!(c.public_url, "http://localhost:8080");
    assert!(c.origin_allowed("http://localhost:8080"));

    // A public address must be https; plain http only for localhost.
    assert!(lookup(&[("ACCOUNTS_PUBLIC_URL", "http://example.com")]).is_err());
    assert!(lookup(&[("ACCOUNTS_PUBLIC_URL", "http://127.0.0.1:9000")]).is_ok());
    let c = lookup(&[("ACCOUNTS_PUBLIC_URL", "https://api.example.com/"), ("ACCOUNTS_ALLOWED_ORIGINS", "https://codlegacy.pages.dev/, https://codlegacy.example")])
        .unwrap();
    assert_eq!(c.public_url, "https://api.example.com");
    assert!(c.origin_allowed("https://codlegacy.pages.dev"));
    assert!(c.origin_allowed("https://api.example.com"));
    assert!(!c.origin_allowed("https://evil.test"));

    // Server keys must be long enough to be worth having.
    assert!(lookup(&[("ACCOUNTS_SERVER_KEYS", "eu1:short")]).is_err());
    assert!(lookup(&[("ACCOUNTS_SERVER_KEYS", "nocolon")]).is_err());
    let c = lookup(&[("ACCOUNTS_SERVER_KEYS", "eu1:0123456789012345678901234,us1:abcdefghijklmnopqrstuvwxyz")]).unwrap();
    assert_eq!(c.server_keys.len(), 2);
    assert_ne!(c.server_keys[0].1, "0123456789012345678901234", "keys are kept hashed");
    let _ = db::open(":memory:").unwrap();
}
