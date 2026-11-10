//! Talks to the account service with the launcher key.

use std::time::Duration;

use serde::Deserialize;

use crate::error::{Error, Fault};
use crate::manifest::Manifest;
use crate::settings::{self, Settings};
use crate::signing;

pub struct Client {
    base: String,
    key: String,
    agent: ureq::Agent,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Me {
    pub id: i64,
    pub callsign: String,
    #[serde(default)]
    pub steam_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Ticket {
    pub ticket: String,
    pub expires_in: u64,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ServerRow {
    pub name: String,
    pub region: String,
    pub mode: String,
    pub map: String,
    pub players: i64,
    pub max_players: i64,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct NewsItem {
    pub slug: String,
    #[serde(default)]
    pub kind: String,
    pub date: String,
    pub title: String,
    #[serde(default)]
    pub summary: String,
}

impl Client {
    pub fn new(s: &Settings) -> Result<Client, Error> {
        let base = settings::normalise_api(&s.api_url)?;
        // A service on this PC (testing) is never reached through a proxy.
        let local = url::Url::parse(&base).ok().and_then(|u| u.host_str().map(|h| matches!(h, "localhost" | "127.0.0.1" | "[::1]"))).unwrap_or(false);
        let mut cfg = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(12))).http_status_as_error(false);
        if local {
            cfg = cfg.proxy(None);
        }
        let agent: ureq::Agent = cfg.build().into();
        Ok(Client { base, key: s.key.clone(), agent })
    }

    fn unreachable(e: impl std::fmt::Display) -> Error {
        Error::new(Fault::Unreachable, format!("Could not reach the CodLegacy account service ({e})."))
    }

    fn get_json<T: for<'de> Deserialize<'de>>(&self, path: &str, auth: bool) -> Result<T, Error> {
        let mut req = self.agent.get(format!("{}{path}", self.base));
        if auth {
            req = req.header("Authorization", format!("Bearer {}", self.key));
        }
        let mut resp = req.call().map_err(Self::unreachable)?;
        self.read(&mut resp)
    }

    fn read<T: for<'de> Deserialize<'de>>(&self, resp: &mut ureq::http::Response<ureq::Body>) -> Result<T, Error> {
        let status = resp.status().as_u16();
        if status == 401 {
            return Err(Error::new(Fault::KeyCancelled, "That launcher key is not valid any more. Make a new one on your account page."));
        }
        if status == 429 {
            return Err(Error::new(Fault::TicketRefused, "Too many tries. Wait a minute and press Play again."));
        }
        if !(200..300).contains(&status) {
            return Err(Error::other(format!("The account service answered {status}.")));
        }
        let text = resp.body_mut().read_to_string().map_err(Self::unreachable)?;
        serde_json::from_str(&text).map_err(|e| Error::other(format!("Unexpected answer from the account service ({e}).")))
    }

    /// Who the key belongs to. This is also the "is my key good" check.
    pub fn me(&self) -> Result<Me, Error> {
        self.get_json("/me", true)
    }

    /// A one-minute, single-use ticket for the game to present to a server.
    pub fn join_ticket(&self) -> Result<Ticket, Error> {
        let mut resp = self
            .agent
            .post(format!("{}/join-ticket", self.base))
            .header("Authorization", format!("Bearer {}", self.key))
            .send_empty()
            .map_err(Self::unreachable)?;
        self.read(&mut resp)
    }

    pub fn servers(&self) -> Result<Vec<ServerRow>, Error> {
        #[derive(Deserialize)]
        struct W {
            servers: Vec<ServerRow>,
        }
        Ok(self.get_json::<W>("/servers", false)?.servers)
    }

    pub fn news(&self) -> Result<Vec<NewsItem>, Error> {
        #[derive(Deserialize)]
        struct W {
            updates: Vec<NewsItem>,
        }
        Ok(self.get_json::<W>("/data/updates.json", false)?.updates)
    }

    fn get_text(&self, path: &str) -> Result<String, Error> {
        let mut resp = self.agent.get(format!("{}{path}", self.base)).call().map_err(Self::unreachable)?;
        if resp.status().as_u16() != 200 {
            return Err(Error::other("No build has been published yet."));
        }
        resp.body_mut().read_to_string().map_err(Self::unreachable)
    }

    /// A manifest (`manifest` for the game build, `launcher` for this program), checked against
    /// the signing key built into the launcher before anything in it is believed.
    pub fn signed_manifest(&self, name: &str) -> Result<Manifest, Error> {
        let text = self.get_text(&format!("/data/build/{name}.json"))?;
        let sig = self.get_text(&format!("/data/build/{name}.json.sig"))?;
        signing::verify_embedded(text.as_bytes(), &sig)?;
        Manifest::parse(&text)
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    /// Downloads one file of the build, at most `limit` bytes.
    pub fn download(&self, url: &str, limit: u64) -> Result<Vec<u8>, Error> {
        use std::io::Read;
        let mut resp = self.agent.get(url).call().map_err(Self::unreachable)?;
        if resp.status().as_u16() != 200 {
            return Err(Error::other(format!("Download failed ({}): {url}", resp.status())));
        }
        let mut out = Vec::new();
        resp.body_mut().as_reader().take(limit + 1).read_to_end(&mut out).map_err(Self::unreachable)?;
        if out.len() as u64 > limit {
            return Err(Error::new(Fault::BadChecksum, "A download was larger than the manifest said."));
        }
        Ok(out)
    }
}
