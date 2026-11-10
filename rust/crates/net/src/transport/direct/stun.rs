//! A STUN binding request (RFC 5389): the public address a UDP socket's
//! packets leave the NAT from, asked of public STUN servers before the
//! socket is handed to QUIC.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::{Duration, Instant};

const MAGIC: u32 = 0x2112_A442;

/// The servers asked, in order (`CODLEGACY_P2P_STUN`: a comma list overrides).
pub fn servers() -> Vec<String> {
    match codlegacy_env::var("CODLEGACY_P2P_STUN") {
        Ok(list) if !list.trim().is_empty() => list
            .split(',')
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect(),
        _ => ["stun.l.google.com:19302", "stun1.l.google.com:19302", "stun.cloudflare.com:3478"]
            .map(str::to_owned)
            .to_vec(),
    }
}

/// Ask each server in turn until one answers within `wait`: the socket's
/// public IPv4 address, and the server that told it.
pub fn public_v4(socket: &UdpSocket, wait: Duration) -> Option<(SocketAddr, SocketAddr)> {
    let started = Instant::now();
    let previous = socket.read_timeout().ok().flatten();
    let mut out = None;
    'servers: for name in servers() {
        let Some(server) = name.to_socket_addrs().ok().and_then(|mut a| a.find(SocketAddr::is_ipv4)) else {
            continue;
        };
        let target = to_socket_family(socket, server);
        let mut txid = [0_u8; 12];
        if getrandom::fill(&mut txid).is_err() {
            break;
        }
        let mut request = Vec::with_capacity(20);
        request.extend_from_slice(&1_u16.to_be_bytes());
        request.extend_from_slice(&0_u16.to_be_bytes());
        request.extend_from_slice(&MAGIC.to_be_bytes());
        request.extend_from_slice(&txid);
        for _ in 0..3 {
            let left = wait.saturating_sub(started.elapsed());
            if left.is_zero() {
                break 'servers;
            }
            let _ = socket.send_to(&request, target);
            let slice = left.min(Duration::from_millis(400));
            let _ = socket.set_read_timeout(Some(slice));
            let until = Instant::now() + slice;
            let mut buf = [0_u8; 512];
            while Instant::now() < until {
                let Ok((n, _)) = socket.recv_from(&mut buf) else { break };
                if let Some(addr) = parse(&buf[..n], &txid) {
                    out = Some((unmap(addr), server));
                    break 'servers;
                }
            }
        }
    }
    let _ = socket.set_read_timeout(previous);
    out
}

/// A dual-stack socket sends to an IPv4 server at its mapped address.
pub fn to_socket_family(socket: &UdpSocket, to: SocketAddr) -> SocketAddr {
    match (socket.local_addr(), to) {
        (Ok(SocketAddr::V6(_)), SocketAddr::V4(v4)) => {
            SocketAddr::new(IpAddr::V6(v4.ip().to_ipv6_mapped()), v4.port())
        }
        _ => to,
    }
}

/// An IPv4-mapped IPv6 address as the IPv4 one.
pub fn unmap(addr: SocketAddr) -> SocketAddr {
    match addr {
        SocketAddr::V6(v6) => match v6.ip().to_ipv4_mapped() {
            Some(v4) => SocketAddr::new(IpAddr::V4(v4), v6.port()),
            None => addr,
        },
        SocketAddr::V4(_) => addr,
    }
}

fn parse(msg: &[u8], txid: &[u8; 12]) -> Option<SocketAddr> {
    if msg.len() < 20 || msg[0..2] != [0x01, 0x01] || msg[4..8] != MAGIC.to_be_bytes() || &msg[8..20] != txid {
        return None;
    }
    let len = usize::from(u16::from_be_bytes([msg[2], msg[3]]));
    let body = msg.get(20..20 + len)?;
    let mut at = 0;
    let mut plain = None;
    while at + 4 <= body.len() {
        let kind = u16::from_be_bytes([body[at], body[at + 1]]);
        let n = usize::from(u16::from_be_bytes([body[at + 2], body[at + 3]]));
        let value = body.get(at + 4..at + 4 + n)?;
        match kind {
            0x0020 => return address(value, true, txid),
            0x0001 => plain = address(value, false, txid),
            _ => {}
        }
        at += 4 + n.div_ceil(4) * 4;
    }
    plain
}

fn address(v: &[u8], xor: bool, txid: &[u8; 12]) -> Option<SocketAddr> {
    if v.len() < 4 {
        return None;
    }
    let magic = MAGIC.to_be_bytes();
    let mut port = u16::from_be_bytes([v[2], v[3]]);
    if xor {
        port ^= u16::from_be_bytes([magic[0], magic[1]]);
    }
    match v[1] {
        1 if v.len() >= 8 => {
            let mut ip = [v[4], v[5], v[6], v[7]];
            if xor {
                for (b, m) in ip.iter_mut().zip(magic) {
                    *b ^= m;
                }
            }
            Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::from(ip)), port))
        }
        2 if v.len() >= 20 => {
            let mut ip = [0_u8; 16];
            ip.copy_from_slice(&v[4..20]);
            if xor {
                for (b, k) in ip.iter_mut().zip(magic.iter().chain(txid)) {
                    *b ^= k;
                }
            }
            Some(SocketAddr::new(IpAddr::V6(Ipv6Addr::from(ip)), port))
        }
        _ => None,
    }
}
