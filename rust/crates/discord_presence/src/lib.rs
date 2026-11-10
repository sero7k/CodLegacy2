//! Discord Rich Presence over the Discord client's local IPC socket (the
//! protocol Discord's own RPC libraries speak; no SDK, no network).
//!
//! Frames are `opcode: u32 LE`, `length: u32 LE`, then `length` bytes of
//! JSON. Opcode 0 is the handshake (`{"v":1,"client_id":"…"}`), 1 a frame
//! (`SET_ACTIVITY` commands, Discord's replies), 2 close. The socket is
//! `\\.\pipe\discord-ipc-N` on Windows and `discord-ipc-N` under
//! `$XDG_RUNTIME_DIR`, `$TMPDIR`, `$TMP`, `$TEMP` or `/tmp` elsewhere
//! (N 0 to 9, the first that opens).
//!
//! [`Presence`] runs a thread of its own: [`Presence::set`] hands it the
//! latest [`Activity`]; it connects when Discord is running (retrying every
//! 15 s), sends an activity only when it changed and at most once every
//! 4 s (Discord takes 5 updates in 20 s), and clears it on drop.

use std::io::{Read, Write};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// What the player is doing, as Discord shows it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Activity {
    /// First line under the game's name (e.g. "Team Deathmatch on Nuketown").
    pub details: String,
    /// Second line (e.g. "Winning 54 - 32").
    pub state: String,
    /// The large picture's asset key (uploaded to the Discord application)
    /// and its hover text.
    pub large_image: String,
    pub large_text: String,
    /// The small picture's asset key and hover text.
    pub small_image: String,
    pub small_text: String,
    /// Unix seconds: "elapsed" counts from `start`, "left" down to `end`.
    pub start: Option<u64>,
    pub end: Option<u64>,
    /// (in the party, party size).
    pub party: Option<(u32, u32)>,
}

impl Activity {
    /// The `SET_ACTIVITY` command for process `pid`.
    pub fn command(&self, pid: u32, nonce: u64) -> serde_json::Value {
        use serde_json::{Map, Value, json};
        let mut a = Map::new();
        let put = |a: &mut Map<String, Value>, k: &str, v: &str| {
            if !v.is_empty() {
                // (Discord takes 2 to 128 characters.)
                let mut s: String = v.chars().take(128).collect();
                if s.chars().count() < 2 {
                    s.push(' ');
                }
                a.insert(k.to_owned(), Value::String(s));
            }
        };
        put(&mut a, "details", &self.details);
        put(&mut a, "state", &self.state);
        let mut assets = Map::new();
        put(&mut assets, "large_image", &self.large_image);
        put(&mut assets, "large_text", &self.large_text);
        put(&mut assets, "small_image", &self.small_image);
        put(&mut assets, "small_text", &self.small_text);
        if !assets.is_empty() {
            a.insert("assets".into(), Value::Object(assets));
        }
        let mut ts = Map::new();
        if let Some(s) = self.start {
            ts.insert("start".into(), json!(s));
        }
        if let Some(e) = self.end {
            ts.insert("end".into(), json!(e));
        }
        if !ts.is_empty() {
            a.insert("timestamps".into(), Value::Object(ts));
        }
        if let Some((n, size)) = self.party.filter(|p| p.1 > 0) {
            a.insert("party".into(), json!({ "id": "legacy-match", "size": [n.min(size).max(1), size] }));
        }
        json!({ "cmd": "SET_ACTIVITY", "args": { "pid": pid, "activity": Value::Object(a) }, "nonce": nonce.to_string() })
    }
}

/// One encoded IPC frame.
pub fn frame(op: u32, body: &serde_json::Value) -> Vec<u8> {
    let data = body.to_string().into_bytes();
    let mut out = Vec::with_capacity(8 + data.len());
    out.extend_from_slice(&op.to_le_bytes());
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(&data);
    out
}

trait Pipe: Read + Write + Send {}
impl<T: Read + Write + Send> Pipe for T {}

#[cfg(windows)]
fn open(n: u32) -> std::io::Result<Box<dyn Pipe>> {
    let f = std::fs::OpenOptions::new().read(true).write(true).open(format!(r"\\.\pipe\discord-ipc-{n}"))?;
    Ok(Box::new(f))
}

#[cfg(unix)]
fn open(n: u32) -> std::io::Result<Box<dyn Pipe>> {
    let mut last = std::io::Error::from(std::io::ErrorKind::NotFound);
    for var in ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"] {
        let Some(dir) = std::env::var_os(var) else { continue };
        let base = std::path::PathBuf::from(dir);
        // (Flatpak and Snap Discords keep theirs in a folder below.)
        for sub in ["", "app/com.discordapp.Discord", "snap.discord"] {
            match std::os::unix::net::UnixStream::connect(base.join(sub).join(format!("discord-ipc-{n}"))) {
                Ok(s) => return Ok(Box::new(s)),
                Err(e) => last = e,
            }
        }
    }
    std::os::unix::net::UnixStream::connect(format!("/tmp/discord-ipc-{n}")).map(|s| Box::new(s) as Box<dyn Pipe>).map_err(|_| last)
}

#[cfg(not(any(unix, windows)))]
fn open(_n: u32) -> std::io::Result<Box<dyn Pipe>> {
    Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
}

fn read_frame(p: &mut dyn Pipe) -> std::io::Result<(u32, serde_json::Value)> {
    let mut head = [0u8; 8];
    p.read_exact(&mut head)?;
    let op = u32::from_le_bytes([head[0], head[1], head[2], head[3]]);
    let len = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize;
    if len > 1 << 20 {
        return Err(std::io::Error::other("frame too long"));
    }
    let mut body = vec![0u8; len];
    p.read_exact(&mut body)?;
    Ok((op, serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null)))
}

/// Connects and shakes hands; `Ok` once Discord sent its READY.
fn connect(client_id: &str) -> Result<Box<dyn Pipe>, String> {
    let mut err = String::from("no Discord socket");
    for n in 0..10 {
        match open(n) {
            Ok(mut p) => {
                p.write_all(&frame(0, &serde_json::json!({ "v": 1, "client_id": client_id }))).map_err(|e| e.to_string())?;
                let (op, body) = read_frame(p.as_mut()).map_err(|e| e.to_string())?;
                if op == 1 && body.get("evt").and_then(|e| e.as_str()) == Some("READY") {
                    return Ok(p);
                }
                return Err(format!("Discord refused the handshake: {body}"));
            }
            Err(e) => err = format!("discord-ipc-{n}: {e}"),
        }
    }
    Err(err)
}

#[derive(Default)]
struct Shared {
    wanted: Option<Activity>,
    stop: bool,
    status: String,
}

/// The presence thread.
pub struct Presence {
    shared: Arc<(Mutex<Shared>, Condvar)>,
    thread: Option<std::thread::JoinHandle<()>>,
}

/// The shortest gap between two activity updates.
const MIN_GAP: Duration = Duration::from_secs(4);
/// How often a missing Discord is looked for again.
const RETRY: Duration = Duration::from_secs(15);

impl Presence {
    /// Starts the thread for Discord application `client_id`.
    pub fn start(client_id: &str) -> Self {
        let shared = Arc::new((Mutex::new(Shared::default()), Condvar::new()));
        let s = shared.clone();
        let id = client_id.to_owned();
        let thread = std::thread::Builder::new().name("discord_presence".into()).spawn(move || run(&id, &s)).ok();
        Self { shared, thread }
    }

    /// The activity to show (sent when it changed, rate-limited).
    pub fn set(&self, activity: Activity) {
        let (m, cv) = &*self.shared;
        if let Ok(mut s) = m.lock()
            && s.wanted.as_ref() != Some(&activity)
        {
            s.wanted = Some(activity);
            cv.notify_one();
        }
    }

    /// What the thread last did (for the log): connected, waiting, why not.
    pub fn status(&self) -> String {
        self.shared.0.lock().map_or_else(|_| String::new(), |s| s.status.clone())
    }
}

impl Drop for Presence {
    fn drop(&mut self) {
        let (m, cv) = &*self.shared;
        if let Ok(mut s) = m.lock() {
            s.stop = true;
        }
        cv.notify_one();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn run(client_id: &str, shared: &(Mutex<Shared>, Condvar)) {
    let (m, cv) = shared;
    let pid = std::process::id();
    let mut pipe: Option<Box<dyn Pipe>> = None;
    let mut sent: Option<Activity> = None;
    let mut last_send = Instant::now() - MIN_GAP;
    let mut last_try = Instant::now() - RETRY;
    let mut nonce = 0u64;
    let set_status = |s: &str| {
        if let Ok(mut g) = m.lock() {
            g.status = s.to_owned();
        }
    };
    loop {
        let (wanted, stop) = {
            let Ok(g) = m.lock() else { return };
            // Wake for a change, or to retry / to send what the rate limit held.
            let g = cv.wait_timeout(g, Duration::from_secs(1)).map(|(g, _)| g);
            let Ok(g) = g else { return };
            (g.wanted.clone(), g.stop)
        };
        if stop {
            if let Some(p) = pipe.as_mut() {
                nonce += 1;
                let clear = serde_json::json!({ "cmd": "SET_ACTIVITY", "args": { "pid": pid }, "nonce": nonce.to_string() });
                let _ = p.write_all(&frame(1, &clear));
                let _ = p.write_all(&frame(2, &serde_json::json!({})));
            }
            return;
        }
        if pipe.is_none() {
            if last_try.elapsed() < RETRY {
                continue;
            }
            last_try = Instant::now();
            match connect(client_id) {
                Ok(p) => {
                    pipe = Some(p);
                    sent = None;
                    set_status("connected to Discord");
                }
                Err(e) => {
                    set_status(&format!("Discord not reached: {e}"));
                    continue;
                }
            }
        }
        let Some(want) = wanted else { continue };
        if sent.as_ref() == Some(&want) || last_send.elapsed() < MIN_GAP {
            continue;
        }
        nonce += 1;
        let Some(p) = pipe.as_mut() else { continue };
        let ok = p.write_all(&frame(1, &want.command(pid, nonce))).is_ok() && read_frame(p.as_mut()).is_ok_and(|(op, body)| op == 1 && body.get("evt").is_none_or(|e| !e.is_string() || e.as_str() != Some("ERROR")));
        if ok {
            sent = Some(want);
            last_send = Instant::now();
            set_status("activity shown on Discord");
        } else {
            // (Discord closed or refused it: reconnect later.)
            pipe = None;
            set_status("lost Discord; retrying");
        }
    }
}

/// Seconds since the Unix epoch.
pub fn unix_now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_carry_opcode_length_and_json() {
        let f = frame(0, &serde_json::json!({ "v": 1 }));
        assert_eq!(&f[..4], &0u32.to_le_bytes());
        assert_eq!(u32::from_le_bytes([f[4], f[5], f[6], f[7]]) as usize, f.len() - 8);
        assert_eq!(&f[8..], br#"{"v":1}"#);
    }

    #[test]
    fn the_activity_command_holds_what_is_set() {
        let a = Activity { details: "Team Deathmatch on Riot".into(), state: "Winning 54 - 32".into(), large_image: "legacy_logo".into(), start: Some(100), party: Some((3, 12)), ..Default::default() };
        let c = a.command(42, 7);
        assert_eq!(c["cmd"], "SET_ACTIVITY");
        assert_eq!(c["args"]["pid"], 42);
        let act = &c["args"]["activity"];
        assert_eq!(act["details"], "Team Deathmatch on Riot");
        assert_eq!(act["assets"]["large_image"], "legacy_logo");
        assert_eq!(act["timestamps"]["start"], 100);
        assert_eq!(act["party"]["size"][1], 12);
        assert!(act.get("small_image").is_none());
    }
}
