//! Steam sign-in over OpenID 2.0.
//!
//! The player is sent to Steam, signs in there (this service never sees a
//! Steam password), and Steam sends the browser back with a signed assertion
//! naming their 64-bit Steam ID. We check the assertion ourselves, then ask
//! Steam to confirm its signature (`check_authentication`). No Steam Web API
//! key is needed for this.

use std::collections::HashMap;

pub const OPENID_ENDPOINT: &str = "https://steamcommunity.com/openid/login";
const NS: &str = "http://specs.openid.net/auth/2.0";
const IDENTIFIER_SELECT: &str = "http://specs.openid.net/auth/2.0/identifier_select";

/// What Steam must sign for us to accept the assertion.
const REQUIRED_SIGNED: [&str; 6] = ["op_endpoint", "claimed_id", "identity", "return_to", "response_nonce", "assoc_handle"];

/// The address Steam sends the browser back to, carrying our one-time `state`.
pub fn return_to(public_url: &str, state: &str) -> String {
    format!("{public_url}/auth/steam/callback?state={state}")
}

/// The Steam sign-in page URL for a given state.
pub fn auth_url(public_url: &str, state: &str) -> String {
    let rt = return_to(public_url, state);
    let q = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("openid.ns", NS)
        .append_pair("openid.mode", "checkid_setup")
        .append_pair("openid.return_to", &rt)
        .append_pair("openid.realm", public_url)
        .append_pair("openid.identity", IDENTIFIER_SELECT)
        .append_pair("openid.claimed_id", IDENTIFIER_SELECT)
        .finish();
    format!("{OPENID_ENDPOINT}?{q}")
}

pub struct Assertion {
    pub steam_id: String,
    pub nonce: String,
}

/// Checks that need no network. Returns the Steam ID and the response nonce.
pub fn check_local(q: &HashMap<String, String>, expected_return_to: &str) -> Result<Assertion, &'static str> {
    let get = |k: &str| q.get(&format!("openid.{k}")).map(String::as_str);
    if get("ns") != Some(NS) || get("mode") != Some("id_res") {
        return Err("steam_invalid");
    }
    if get("op_endpoint") != Some(OPENID_ENDPOINT) {
        return Err("steam_invalid");
    }
    // Bound to the sign-in we started: not replayable on another state or site.
    if get("return_to") != Some(expected_return_to) {
        return Err("steam_invalid");
    }
    let signed: Vec<&str> = get("signed").unwrap_or("").split(',').collect();
    if !REQUIRED_SIGNED.iter().all(|f| signed.contains(f)) {
        return Err("steam_invalid");
    }
    let claimed = get("claimed_id").ok_or("steam_invalid")?;
    if get("identity") != Some(claimed) {
        return Err("steam_invalid");
    }
    let id = claimed
        .strip_prefix("https://steamcommunity.com/openid/id/")
        .or_else(|| claimed.strip_prefix("http://steamcommunity.com/openid/id/"))
        .ok_or("steam_invalid")?;
    if id.len() != 17 || !id.bytes().all(|b| b.is_ascii_digit()) || !id.starts_with("7656") {
        return Err("steam_invalid");
    }
    let nonce = get("response_nonce").ok_or("steam_invalid")?;
    if nonce.is_empty() || nonce.len() > 128 {
        return Err("steam_invalid");
    }
    Ok(Assertion { steam_id: id.to_string(), nonce: nonce.to_string() })
}

/// The form to post back to Steam to have it confirm its own signature.
pub fn check_form(q: &HashMap<String, String>) -> Vec<(String, String)> {
    let mut form: Vec<(String, String)> = q
        .iter()
        .filter(|(k, _)| k.starts_with("openid.") && k.as_str() != "openid.mode")
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    form.sort();
    form.push(("openid.mode".into(), "check_authentication".into()));
    form
}

/// Confirms an assertion with Steam. A trait so tests can stand in for Steam.
pub trait SteamVerifier: Send + Sync + 'static {
    /// Blocking. `Ok(true)` only if Steam says the signature is valid.
    fn check_authentication(&self, form: &[(String, String)]) -> Result<bool, String>;
}

pub struct SteamHttp;

impl SteamVerifier for SteamHttp {
    fn check_authentication(&self, form: &[(String, String)]) -> Result<bool, String> {
        let body = url::form_urlencoded::Serializer::new(String::new()).extend_pairs(form).finish();
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(std::time::Duration::from_secs(10)))
            .build()
            .into();
        let mut resp = agent
            .post(OPENID_ENDPOINT)
            .content_type("application/x-www-form-urlencoded")
            .send(body.as_str())
            .map_err(|e| e.to_string())?;
        let text = resp.body_mut().read_to_string().map_err(|e| e.to_string())?;
        Ok(text.lines().any(|l| l.trim() == "is_valid:true"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn good(state: &str) -> HashMap<String, String> {
        let rt = return_to("https://api.example.test", state);
        let mut m = HashMap::new();
        for (k, v) in [
            ("ns", NS),
            ("mode", "id_res"),
            ("op_endpoint", OPENID_ENDPOINT),
            ("claimed_id", "https://steamcommunity.com/openid/id/76561198000000001"),
            ("identity", "https://steamcommunity.com/openid/id/76561198000000001"),
            ("return_to", rt.as_str()),
            ("response_nonce", "2026-11-05T07:00:00ZabcDEF"),
            ("assoc_handle", "1234567890"),
            ("signed", "signed,op_endpoint,claimed_id,identity,return_to,response_nonce,assoc_handle"),
            ("sig", "c2ln"),
        ] {
            m.insert(format!("openid.{k}"), v.to_string());
        }
        m
    }

    #[test]
    fn accepts_a_well_formed_assertion() {
        let rt = return_to("https://api.example.test", "s1");
        let a = check_local(&good("s1"), &rt).unwrap();
        assert_eq!(a.steam_id, "76561198000000001");
    }

    #[test]
    fn rejects_tampering() {
        let rt = return_to("https://api.example.test", "s1");
        let mut m = good("s1");
        m.insert("openid.claimed_id".into(), "https://evil.test/openid/id/76561198000000001".into());
        m.insert("openid.identity".into(), "https://evil.test/openid/id/76561198000000001".into());
        assert!(check_local(&m, &rt).is_err());

        let mut m = good("s1");
        m.insert("openid.op_endpoint".into(), "https://evil.test/openid/login".into());
        assert!(check_local(&m, &rt).is_err());

        // Valid for another state: not accepted here.
        assert!(check_local(&good("s2"), &rt).is_err());

        let mut m = good("s1");
        m.insert("openid.signed".into(), "signed,op_endpoint".into());
        assert!(check_local(&m, &rt).is_err());

        let mut m = good("s1");
        m.insert("openid.identity".into(), "https://steamcommunity.com/openid/id/76561198000000002".into());
        assert!(check_local(&m, &rt).is_err());

        let mut m = good("s1");
        m.insert("openid.claimed_id".into(), "https://steamcommunity.com/openid/id/12345".into());
        m.insert("openid.identity".into(), "https://steamcommunity.com/openid/id/12345".into());
        assert!(check_local(&m, &rt).is_err());
    }

    #[test]
    fn check_form_swaps_the_mode() {
        let f = check_form(&good("s1"));
        assert_eq!(f.last().unwrap(), &("openid.mode".to_string(), "check_authentication".to_string()));
        assert_eq!(f.iter().filter(|(k, _)| k == "openid.mode").count(), 1);
        assert!(f.iter().any(|(k, _)| k == "openid.sig"));
    }

    #[test]
    fn auth_url_names_the_return_address() {
        let u = auth_url("https://api.example.test", "abc");
        assert!(u.starts_with(OPENID_ENDPOINT));
        assert!(u.contains("openid.mode=checkid_setup"));
        assert!(u.contains("state%3Dabc"));
    }
}
