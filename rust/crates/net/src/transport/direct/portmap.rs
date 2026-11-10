//! Opening the host's UDP port on his router: UPnP IGD (SSDP, then the
//! router's WAN connection service by SOAP), else NAT-PMP. Best effort: a
//! router with neither leaves hole punching or a forwarded port.

use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream, UdpSocket};
use std::time::{Duration, Instant};

const SSDP: &str = "239.255.255.250:1900";
const LEASE_S: u32 = 7200;
const DESCRIPTION: &str = "BO2 custom game";

#[derive(Clone, Debug)]
pub enum Mapping {
    Upnp { control: String, service: String, port: u16 },
    NatPmp { gateway: Ipv4Addr, port: u16 },
}

impl Mapping {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Upnp { .. } => "UPnP",
            Self::NatPmp { .. } => "NAT-PMP",
        }
    }

    pub fn external_port(&self) -> u16 {
        match self {
            Self::Upnp { port, .. } | Self::NatPmp { port, .. } => *port,
        }
    }

    /// Take the mapping back off the router.
    pub fn remove(&self) {
        match self {
            Self::Upnp { control, service, port } => {
                let body = format!(
                    "<NewRemoteHost></NewRemoteHost><NewExternalPort>{port}</NewExternalPort><NewProtocol>UDP</NewProtocol>"
                );
                let _ = soap(control, service, "DeletePortMapping", &body);
            }
            Self::NatPmp { gateway, port } => {
                let _ = natpmp(*gateway, *port, 0);
            }
        }
    }
}

/// Map UDP `port` on the router to this PC's same port.
pub fn map(port: u16) -> Option<Mapping> {
    let gateway = match upnp(port) {
        Ok(mapping) => return Some(mapping),
        Err(gateway) => gateway,
    };
    let gateway = gateway.or_else(|| {
        let lan = super::lan_v4()?;
        let o = lan.octets();
        Some(Ipv4Addr::new(o[0], o[1], o[2], 1))
    })?;
    let external = natpmp(gateway, port, LEASE_S)?;
    Some(Mapping::NatPmp { gateway, port: external })
}

/// UPnP: the mapping, or the router's address when it answered SSDP but
/// would not map.
fn upnp(port: u16) -> Result<Mapping, Option<Ipv4Addr>> {
    let location = discover().ok_or(None)?;
    let router = host_of(&location).and_then(|h| h.split(':').next()?.parse::<Ipv4Addr>().ok());
    let description = http("GET", &location, &[], "").ok_or(router)?;
    let (service, control) = wan_service(&description, &location).ok_or(router)?;
    let lan = local_ip_towards(&location).ok_or(router)?;
    for lease in [LEASE_S, 0] {
        let body = format!(
            "<NewRemoteHost></NewRemoteHost><NewExternalPort>{port}</NewExternalPort><NewProtocol>UDP</NewProtocol>\
             <NewInternalPort>{port}</NewInternalPort><NewInternalClient>{lan}</NewInternalClient><NewEnabled>1</NewEnabled>\
             <NewPortMappingDescription>{DESCRIPTION}</NewPortMappingDescription><NewLeaseDuration>{lease}</NewLeaseDuration>"
        );
        if soap(&control, &service, "AddPortMapping", &body).is_some() {
            return Ok(Mapping::Upnp { control, service, port });
        }
    }
    Err(router)
}

fn discover() -> Option<String> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    let started = Instant::now();
    for st in [
        "urn:schemas-upnp-org:device:InternetGatewayDevice:1",
        "urn:schemas-upnp-org:device:InternetGatewayDevice:2",
        "urn:schemas-upnp-org:service:WANIPConnection:1",
    ] {
        let search = format!(
            "M-SEARCH * HTTP/1.1\r\nHOST: {SSDP}\r\nST: {st}\r\nMAN: \"ssdp:discover\"\r\nMX: 2\r\n\r\n"
        );
        let _ = socket.send_to(search.as_bytes(), SSDP);
    }
    let mut buf = [0_u8; 2048];
    while started.elapsed() < Duration::from_millis(2500) {
        let _ = socket.set_read_timeout(Some(Duration::from_millis(2500).saturating_sub(started.elapsed()).max(Duration::from_millis(10))));
        let Ok((n, _)) = socket.recv_from(&mut buf) else { break };
        let text = String::from_utf8_lossy(&buf[..n]);
        if let Some(location) = header(&text, "location") {
            return Some(location);
        }
    }
    None
}

fn header(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let (k, v) = l.split_once(':')?;
        k.trim().eq_ignore_ascii_case(name).then(|| v.trim().to_owned())
    })
}

fn host_of(url: &str) -> Option<&str> {
    url.strip_prefix("http://")?.split('/').next()
}

fn wan_service(xml: &str, location: &str) -> Option<(String, String)> {
    for kind in [
        "urn:schemas-upnp-org:service:WANIPConnection:2",
        "urn:schemas-upnp-org:service:WANIPConnection:1",
        "urn:schemas-upnp-org:service:WANPPPConnection:1",
    ] {
        let Some(at) = xml.find(&format!("<serviceType>{kind}</serviceType>")) else { continue };
        let rest = &xml[at..];
        let end = rest.find("</service>").unwrap_or(rest.len());
        let block = &rest[..end];
        let control = tag(block, "controlURL")?;
        let url = if control.starts_with("http://") {
            control
        } else {
            let base = tag(xml, "URLBase").unwrap_or_else(|| format!("http://{}", host_of(location).unwrap_or("")));
            format!("{}/{}", base.trim_end_matches('/'), control.trim_start_matches('/'))
        };
        return Some((kind.to_owned(), url));
    }
    None
}

fn tag(xml: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&format!("</{name}>"))? + start;
    Some(xml[start..end].trim().to_owned())
}

fn local_ip_towards(url: &str) -> Option<IpAddr> {
    let host = host_of(url)?;
    let addr: SocketAddr = if host.contains(':') { host.parse().ok()? } else { format!("{host}:80").parse().ok()? };
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect(addr).ok()?;
    Some(socket.local_addr().ok()?.ip())
}

fn soap(control: &str, service: &str, action: &str, args: &str) -> Option<String> {
    let body = format!(
        "<?xml version=\"1.0\"?><s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
         s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\"><s:Body><u:{action} xmlns:u=\"{service}\">{args}</u:{action}></s:Body></s:Envelope>"
    );
    let action_header = format!("\"{service}#{action}\"");
    http(
        "POST",
        control,
        &[("Content-Type", "text/xml; charset=\"utf-8\""), ("SOAPAction", &action_header)],
        &body,
    )
}

/// A plain HTTP/1.1 request; the body when the answer is 200.
fn http(method: &str, url: &str, headers: &[(&str, &str)], body: &str) -> Option<String> {
    let rest = url.strip_prefix("http://")?;
    let (host, path) = rest.split_once('/').map_or((rest, "/".to_owned()), |(h, p)| (h, format!("/{p}")));
    let addr: SocketAddr = if host.contains(':') { host.parse().ok()? } else { format!("{host}:80").parse().ok()? };
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(3))).ok()?;
    let mut request = format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\nContent-Length: {}\r\n", body.len());
    for (k, v) in headers {
        request.push_str(&format!("{k}: {v}\r\n"));
    }
    request.push_str("\r\n");
    request.push_str(body);
    stream.write_all(request.as_bytes()).ok()?;
    let mut out = Vec::new();
    let _ = stream.take(256 * 1024).read_to_end(&mut out);
    let text = String::from_utf8_lossy(&out).into_owned();
    let status = text.lines().next()?;
    status.split_whitespace().nth(1).filter(|c| *c == "200")?;
    Some(text.split_once("\r\n\r\n").map_or(String::new(), |(_, b)| b.to_owned()))
}

/// NAT-PMP (RFC 6886) UDP mapping: the external port, `lifetime` 0 removes.
fn natpmp(gateway: Ipv4Addr, port: u16, lifetime: u32) -> Option<u16> {
    let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect((gateway, 5351)).ok()?;
    let mut request = vec![0_u8, 1, 0, 0];
    request.extend_from_slice(&port.to_be_bytes());
    request.extend_from_slice(&port.to_be_bytes());
    request.extend_from_slice(&lifetime.to_be_bytes());
    let mut wait = Duration::from_millis(250);
    for _ in 0..3 {
        socket.send(&request).ok()?;
        socket.set_read_timeout(Some(wait)).ok()?;
        let mut buf = [0_u8; 16];
        if let Ok(n) = socket.recv(&mut buf)
            && n >= 16
            && buf[1] == 129
            && buf[2..4] == [0, 0]
        {
            return Some(u16::from_be_bytes([buf[10], buf[11]]));
        }
        wait *= 2;
    }
    None
}
