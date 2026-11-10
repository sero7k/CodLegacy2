//! bo2mp: peer-to-peer custom games, with no server of ours anywhere.
//!
//! The host's game runs the master service itself (`codlegacy_master::
//! run_embedded`) on a UDP port it opens on his router (UPnP or NAT-PMP,
//! `portmap`) and whose public address it asks of a public STUN server
//! (`stun`). Everything a relay game has then works unchanged: his own game
//! and his friends' connect to that service, his over loopback, theirs
//! straight to his PC, and the room, the map offer, the bootstrap and the
//! game's packets run over those connections.
//!
//! What a friend needs is in the INVITE CODE: the host's addresses (public,
//! LAN, IPv6), the service's secret (sent as the TLS server name: a
//! connection without it is closed before it sees anything) and the
//! fingerprint of the host's certificate (so the friend knows it is his).
//! The friend tries every address at once, the LAN one for a friend on the
//! same network, and a few ports either side of the public one for routers
//! that move ports; the first to answer is the game's.

pub mod portmap;
pub mod stun;

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6, UdpSocket};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use bevy::prelude::*;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use tokio_util::sync::CancellationToken;

/// The port the host opens unless `CODLEGACY_P2P_PORT` names another.
pub const DEFAULT_PORT: u16 = 28970;
/// How long a friend's game tries the host's addresses.
pub const CONNECT_WAIT: Duration = Duration::from_secs(5);
const STUN_WAIT: Duration = Duration::from_millis(1500);
const KEEP_OPEN: Duration = Duration::from_secs(20);
const VERSION: u8 = 1;
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// What a friend's game needs to reach the host's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invite {
    pub token: [u8; 6],
    pub pin: [u8; 12],
    pub public: Option<SocketAddrV4>,
    pub lan: Option<SocketAddrV4>,
    pub v6: Option<SocketAddrV6>,
}

impl Invite {
    /// The service's secret, as the TLS server name.
    pub fn server_name(&self) -> String {
        format!("p2p-{}.iw4l", base32(&self.token).to_ascii_lowercase())
    }

    /// The port to forward when nothing else gets through.
    pub fn port(&self) -> u16 {
        self.public.map(|a| a.port()).or(self.lan.map(|a| a.port())).or(self.v6.map(|a| a.port())).unwrap_or(DEFAULT_PORT)
    }

    pub fn encode(&self) -> String {
        let flags = u8::from(self.public.is_some()) | u8::from(self.lan.is_some()) << 1 | u8::from(self.v6.is_some()) << 2;
        let mut b = vec![VERSION << 4 | flags];
        b.extend_from_slice(&self.token);
        b.extend_from_slice(&self.pin);
        for a in [self.public, self.lan].into_iter().flatten() {
            b.extend_from_slice(&a.ip().octets());
            b.extend_from_slice(&a.port().to_be_bytes());
        }
        if let Some(a) = self.v6 {
            b.extend_from_slice(&a.ip().octets());
            b.extend_from_slice(&a.port().to_be_bytes());
        }
        b.push(crc8(&b));
        base32(&b)
            .as_bytes()
            .chunks(5)
            .map(|c| String::from_utf8_lossy(c).into_owned())
            .collect::<Vec<_>>()
            .join("-")
    }

    pub fn decode(code: &str) -> Result<Self, &'static str> {
        let b = unbase32(code).ok_or("that is not an invite code")?;
        let (&check, body) = b.split_last().ok_or("that invite code is too short")?;
        if crc8(body) != check {
            return Err("that invite code has a mistake in it");
        }
        let (&head, mut rest) = body.split_first().ok_or("that invite code is too short")?;
        if head >> 4 != VERSION {
            return Err("that invite code is from another version of the game");
        }
        let mut take = |n: usize| -> Result<&[u8], &'static str> {
            let (a, r) = rest.split_at_checked(n).ok_or("that invite code is too short")?;
            rest = r;
            Ok(a)
        };
        let token: [u8; 6] = take(6)?.try_into().map_err(|_| "bad code")?;
        let pin: [u8; 12] = take(12)?.try_into().map_err(|_| "bad code")?;
        let mut v4 = |on: bool| -> Result<Option<SocketAddrV4>, &'static str> {
            if !on {
                return Ok(None);
            }
            let a = take(6)?;
            Ok(Some(SocketAddrV4::new(Ipv4Addr::new(a[0], a[1], a[2], a[3]), u16::from_be_bytes([a[4], a[5]]))))
        };
        let public = v4(head & 1 != 0)?;
        let lan = v4(head & 2 != 0)?;
        let v6 = if head & 4 != 0 {
            let a = take(18)?;
            let ip: [u8; 16] = a[..16].try_into().map_err(|_| "bad code")?;
            Some(SocketAddrV6::new(Ipv6Addr::from(ip), u16::from_be_bytes([a[16], a[17]]), 0, 0))
        } else {
            None
        };
        if public.is_none() && lan.is_none() && v6.is_none() {
            return Err("that invite code has no address in it");
        }
        Ok(Self { token, pin, public, lan, v6 })
    }

    /// The addresses to try, each with when to try it: the ones in the code
    /// at once, then the public port's neighbours (a router that gives each
    /// destination its own port usually counts up from the last).
    pub fn candidates(&self) -> Vec<(Duration, SocketAddr)> {
        let mut out: Vec<(Duration, SocketAddr)> = Vec::new();
        for a in [self.lan.map(SocketAddr::V4), self.public.map(SocketAddr::V4), self.v6.map(SocketAddr::V6)].into_iter().flatten() {
            out.push((Duration::ZERO, a));
        }
        if let Some(p) = self.public {
            for d in [1_i32, -1, 2, -2, 3, -3] {
                if let Ok(port) = u16::try_from(i32::from(p.port()) + d)
                    && port != 0
                {
                    out.push((Duration::from_millis(600), SocketAddr::V4(SocketAddrV4::new(*p.ip(), port))));
                }
            }
        }
        out
    }
}

fn base32(bytes: &[u8]) -> String {
    let mut out = String::new();
    let (mut acc, mut bits) = (0_u32, 0_u32);
    for &b in bytes {
        acc = acc << 8 | u32::from(b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((acc >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((acc << (5 - bits)) & 31) as usize] as char);
    }
    out
}

fn unbase32(code: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let (mut acc, mut bits) = (0_u32, 0_u32);
    for c in code.chars().filter(|c| !c.is_whitespace() && *c != '-') {
        let c = match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            c => c,
        };
        let v = ALPHABET.iter().position(|&a| a as char == c)? as u32;
        acc = acc << 5 | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

fn crc8(bytes: &[u8]) -> u8 {
    let mut crc = 0_u8;
    for &b in bytes {
        crc ^= b;
        for _ in 0..8 {
            crc = if crc & 0x80 != 0 { crc << 1 ^ 0x07 } else { crc << 1 };
        }
    }
    crc
}

/// This PC's address on its LAN (the one its default route leaves from;
/// nothing is sent).
pub fn lan_v4() -> Option<Ipv4Addr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("8.8.8.8:80").ok()?;
    match s.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if !ip.is_unspecified() && !ip.is_loopback() => Some(ip),
        _ => None,
    }
}

/// This PC's global IPv6 address, if it has one.
pub fn global_v6() -> Option<Ipv6Addr> {
    let s = UdpSocket::bind("[::]:0").ok()?;
    s.connect("[2001:4860:4860::8888]:80").ok()?;
    match s.local_addr().ok()?.ip() {
        IpAddr::V6(ip) if ip.segments()[0] & 0xe000 == 0x2000 => Some(ip),
        _ => None,
    }
}

/// A UDP socket on `port` taking IPv4 and IPv6 (IPv4 only where the system
/// will not do both on one socket).
pub fn bind_udp(port: u16) -> std::io::Result<UdpSocket> {
    use socket2::{Domain, Protocol, Socket, Type};
    let dual = Socket::new(Domain::IPV6, Type::DGRAM, Some(Protocol::UDP)).and_then(|s| {
        s.set_only_v6(false)?;
        s.bind(&SocketAddr::new(IpAddr::V6(Ipv6Addr::UNSPECIFIED), port).into())?;
        Ok(s)
    });
    match dual {
        Ok(s) => Ok(s.into()),
        Err(_) => UdpSocket::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port)),
    }
}

/// An endpoint on `socket` (a server too when `server` is given).
pub fn endpoint_on(socket: UdpSocket, server: Option<quinn::ServerConfig>) -> std::io::Result<quinn::Endpoint> {
    socket.set_nonblocking(true)?;
    quinn::Endpoint::new(quinn::EndpointConfig::default(), server, socket, Arc::new(quinn::TokioRuntime))
}

/// Trusts exactly the certificate whose fingerprint the invite carries.
#[derive(Debug)]
struct Pinned {
    pin: [u8; 12],
    provider: Arc<rustls::crypto::CryptoProvider>,
}

pub fn fingerprint(der: &[u8]) -> [u8; 12] {
    let digest = ring::digest::digest(&ring::digest::SHA256, der);
    let mut pin = [0_u8; 12];
    pin.copy_from_slice(&digest.as_ref()[..12]);
    pin
}

impl rustls::client::danger::ServerCertVerifier for Pinned {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp: &[u8],
        _now: UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        if fingerprint(end_entity) == self.pin {
            Ok(rustls::client::danger::ServerCertVerified::assertion())
        } else {
            Err(rustls::Error::General("not the host's certificate".into()))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.provider.signature_verification_algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.provider.signature_verification_algorithms.supported_schemes()
    }
}

/// TLS for a friend's game: only the host's own certificate.
pub fn pinned_crypto(pin: [u8; 12]) -> rustls::ClientConfig {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    rustls::ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_safe_default_protocol_versions()
        .expect("ring supports TLS 1.3")
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(Pinned { pin, provider }))
        .with_no_client_auth()
}

/// Try every address on its schedule; the first connection made wins.
pub async fn race(
    endpoint: &quinn::Endpoint,
    client: quinn::ClientConfig,
    candidates: Vec<(Duration, SocketAddr)>,
    server_name: &str,
    cancel: &CancellationToken,
) -> Result<quinn::Connection, String> {
    let local_v6 = endpoint.local_addr().is_ok_and(|a| a.is_ipv6());
    let mut tries = tokio::task::JoinSet::new();
    for (delay, addr) in candidates {
        let addr = match addr {
            SocketAddr::V4(v4) if local_v6 => SocketAddr::new(IpAddr::V6(v4.ip().to_ipv6_mapped()), v4.port()),
            SocketAddr::V6(_) if !local_v6 => continue,
            a => a,
        };
        let (endpoint, client, name) = (endpoint.clone(), client.clone(), server_name.to_owned());
        tries.spawn(async move {
            tokio::time::sleep(delay).await;
            let connecting = endpoint.connect_with(client, addr, &name).map_err(|e| e.to_string())?;
            connecting.await.map_err(|e| e.to_string())
        });
    }
    let deadline = tokio::time::sleep(CONNECT_WAIT);
    tokio::pin!(deadline);
    let mut last = String::from("no address answered");
    loop {
        tokio::select! {
            _ = cancel.cancelled() => return Err("cancelled".into()),
            _ = &mut deadline => break,
            done = tries.join_next() => match done {
                None => break,
                Some(Ok(Ok(connection))) => {
                    tries.abort_all();
                    return Ok(connection);
                }
                Some(Ok(Err(e))) => last = e,
                Some(Err(_)) => {}
            },
        }
    }
    tries.abort_all();
    Err(last)
}

/// Where a peer-to-peer game's service is and how to know it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectTarget {
    pub candidates: Vec<(Duration, SocketAddr)>,
    pub pin: [u8; 12],
    pub server_name: String,
    /// The host's port, for the message when nothing gets through.
    pub port: u16,
}

impl From<&Invite> for DirectTarget {
    fn from(invite: &Invite) -> Self {
        Self { candidates: invite.candidates(), pin: invite.pin, server_name: invite.server_name(), port: invite.port() }
    }
}

/// The host's side: what his lobby shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DirectHostInfo {
    /// The code to send friends (none until the public address is asked).
    pub invite: Option<String>,
    pub port: u16,
    /// The router opened the port ("UPnP", "NAT-PMP").
    pub opened: Option<&'static str>,
    /// No public address was found: friends off his network may not get in.
    pub no_public: bool,
    pub error: Option<String>,
}

/// The service a peer-to-peer host runs while his room is open.
#[derive(Resource)]
pub struct DirectHost {
    info: Arc<Mutex<DirectHostInfo>>,
    invite: Arc<Mutex<Option<Invite>>>,
    cancel: CancellationToken,
    worker: Option<JoinHandle<()>>,
    port: u16,
    loopback: SocketAddr,
    pin: [u8; 12],
    server_name: String,
}

impl DirectHost {
    pub fn start() -> Result<Self, String> {
        let wanted = codlegacy_env::var("CODLEGACY_P2P_PORT")
            .ok()
            .and_then(|p| p.trim().parse().ok())
            .unwrap_or(DEFAULT_PORT);
        let socket = bind_udp(wanted)
            .or_else(|_| bind_udp(0))
            .map_err(|e| format!("could not open a UDP port: {e}"))?;
        let local = socket.local_addr().map_err(|e| e.to_string())?;
        let port = local.port();
        let loopback = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port);
        let mut token = [0_u8; 6];
        getrandom::fill(&mut token).map_err(|e| e.to_string())?;
        let cert = rcgen::generate_simple_self_signed(vec!["iw4l-p2p".to_owned()]).map_err(|e| e.to_string())?;
        let der = cert.cert.der().clone();
        let pin = fingerprint(&der);
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(cert.key_pair.serialize_der()));
        let server = codlegacy_master::server_config(vec![der], key).map_err(|e| e.to_string())?;
        let template = Invite { token, pin, public: None, lan: None, v6: None };
        let server_name = template.server_name();
        let info = Arc::new(Mutex::new(DirectHostInfo { port, ..default() }));
        let invite = Arc::new(Mutex::new(None));
        let cancel = CancellationToken::new();
        let worker = {
            let (info, invite, cancel, name) = (Arc::clone(&info), Arc::clone(&invite), cancel.clone(), server_name.clone());
            std::thread::Builder::new()
                .name("codlegacy-p2p-host".into())
                .spawn(move || host_worker(socket, server, template, name, info, invite, cancel))
                .map_err(|e| e.to_string())?
        };
        diag::info!(Net, "p2p host: service on UDP port {port}");
        Ok(Self { info, invite, cancel, worker: Some(worker), port, loopback, pin, server_name })
    }

    pub fn info(&self) -> DirectHostInfo {
        self.info.lock().map_or_else(|_| DirectHostInfo::default(), |i| i.clone())
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// His own game's way in: loopback, the same secret and certificate.
    pub fn own_target(&self) -> DirectTarget {
        DirectTarget {
            candidates: vec![(Duration::ZERO, self.loopback)],
            pin: self.pin,
            server_name: self.server_name.clone(),
            port: self.port,
        }
    }

    pub fn invite(&self) -> Option<Invite> {
        self.invite.lock().ok().and_then(|i| i.clone())
    }
}

impl Drop for DirectHost {
    fn drop(&mut self) {
        self.cancel.cancel();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn publish(info: &Mutex<DirectHostInfo>, slot: &Mutex<Option<Invite>>, invite: &Invite) {
    if let Ok(mut i) = info.lock() {
        i.invite = Some(invite.encode());
        i.no_public = invite.public.is_none();
    }
    if let Ok(mut s) = slot.lock() {
        *s = Some(invite.clone());
    }
}

fn host_worker(
    socket: UdpSocket,
    server: quinn::ServerConfig,
    mut invite: Invite,
    server_name: String,
    info: Arc<Mutex<DirectHostInfo>>,
    slot: Arc<Mutex<Option<Invite>>>,
    cancel: CancellationToken,
) {
    let port = socket.local_addr().map_or(0, |a| a.port());
    let stun = stun::public_v4(&socket, STUN_WAIT);
    invite.public = stun.and_then(|(a, _)| match a {
        SocketAddr::V4(v4) => Some(v4),
        SocketAddr::V6(_) => None,
    });
    invite.lan = lan_v4().map(|ip| SocketAddrV4::new(ip, port));
    invite.v6 = socket
        .local_addr()
        .ok()
        .filter(SocketAddr::is_ipv6)
        .and(global_v6())
        .map(|ip| SocketAddrV6::new(ip, port, 0, 0));
    diag::info!(
        Net,
        "p2p host: public {} lan {} ipv6 {}",
        invite.public.map_or("unknown".to_owned(), |a| a.to_string()),
        invite.lan.map_or("none".to_owned(), |a| a.to_string()),
        if invite.v6.is_some() { "yes" } else { "no" }
    );
    publish(&info, &slot, &invite);
    let mapping: Arc<Mutex<Option<portmap::Mapping>>> = Arc::new(Mutex::new(None));
    {
        let (info, slot, mapping, mut invite) = (Arc::clone(&info), Arc::clone(&slot), Arc::clone(&mapping), invite.clone());
        let _ = std::thread::Builder::new().name("codlegacy-p2p-portmap".into()).spawn(move || {
            let Some(m) = portmap::map(port) else {
                diag::info!(Net, "p2p host: the router opened no port (no UPnP / NAT-PMP)");
                return;
            };
            diag::info!(Net, "p2p host: {} opened UDP port {} on the router", m.kind(), m.external_port());
            if let Ok(mut i) = info.lock() {
                i.opened = Some(m.kind());
            }
            if let Some(public) = invite.public.as_mut()
                && public.port() != m.external_port()
            {
                public.set_port(m.external_port());
                publish(&info, &slot, &invite);
            }
            if let Ok(mut s) = mapping.lock() {
                *s = Some(m);
            }
        });
    }
    let runtime = match tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build() {
        Ok(r) => r,
        Err(e) => {
            if let Ok(mut i) = info.lock() {
                i.error = Some(e.to_string());
            }
            return;
        }
    };
    let stun_server = stun.map(|(_, server)| server);
    runtime.block_on(async move {
        let endpoint = match endpoint_on(socket, Some(server)) {
            Ok(e) => e,
            Err(e) => {
                if let Ok(mut i) = info.lock() {
                    i.error = Some(e.to_string());
                }
                return;
            }
        };
        let keep_open = {
            let endpoint = endpoint.clone();
            async move {
                // (A packet out every so often keeps his router's door to
                // this port open while he waits for friends.)
                let Some(server) = stun_server else { return std::future::pending().await };
                let client = quinn::ClientConfig::new(Arc::new(
                    quinn::crypto::rustls::QuicClientConfig::try_from(pinned_crypto([0; 12])).expect("TLS 1.3 config"),
                ));
                let target = match (endpoint.local_addr(), server) {
                    (Ok(SocketAddr::V6(_)), SocketAddr::V4(v4)) => SocketAddr::new(IpAddr::V6(v4.ip().to_ipv6_mapped()), v4.port()),
                    _ => server,
                };
                loop {
                    tokio::time::sleep(KEEP_OPEN).await;
                    if let Ok(c) = endpoint.connect_with(client.clone(), target, "keepalive") {
                        drop(c);
                    }
                }
            }
        };
        tokio::select! {
            _ = cancel.cancelled() => {}
            result = codlegacy_master::run_embedded(endpoint.clone(), server_name) => {
                if let Err(e) = result {
                    diag::warn!(Net, "p2p host: service ended: {e}");
                }
            }
            () = keep_open => {}
        }
        endpoint.close(0_u8.into(), b"host closed");
    });
    if let Some(m) = mapping.lock().ok().and_then(|mut m| m.take()) {
        m.remove();
    }
    diag::info!(Net, "p2p host: service closed");
}
