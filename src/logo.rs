//! Fetch a business logo from a public https URL, for prefill links.
//!
//! The server fetches on the user's behalf (most sites don't allow
//! cross-origin canvas access), so this is hardened against SSRF:
//! - https only, port 443 only, no credentials in the URL;
//! - every DNS answer is checked and only public addresses are dialled
//!   (ureq dials exactly what the resolver returns, so the check is pinned);
//! - redirects are followed here, not by ureq, so every hop is re-checked
//!   with the same rules (at most 3 hops, 8 s overall);
//! - 5 s per request, 1 MB size cap, image type sniffed from the bytes;
//! - "blocked" and "couldn't fetch" look identical to callers, so the
//!   endpoint can't be used to probe which internal names resolve;
//! - no environment proxies, no cookies, nothing stored or logged.

use std::io::Read;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs};
use std::time::{Duration, Instant};

pub const MAX_LOGO_FETCH_BYTES: usize = 1024 * 1024;
const MAX_REDIRECTS: usize = 3;

#[derive(Debug, PartialEq, Eq)]
pub enum LogoError {
    BadUrl(&'static str),
    Blocked,
    Fetch,
    TooLarge,
    NotImage,
}

impl LogoError {
    pub fn message(&self) -> String {
        match self {
            LogoError::BadUrl(why) => format!("Logo link isn't usable: {why}"),
            // Deliberately identical: don't reveal whether a name resolved.
            LogoError::Blocked | LogoError::Fetch => {
                "Couldn't download the logo from that link".into()
            }
            LogoError::TooLarge => "Logo is too large (max 1 MB)".into(),
            LogoError::NotImage => "Logo link isn't a JPEG, PNG, GIF, WebP or SVG image".into(),
        }
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_multicast()
        || a == 0
        || a >= 240 // reserved
        || (a == 100 && (64..=127).contains(&b)) // carrier-grade NAT
        || (a == 192 && b == 0 && c == 0) // IETF protocol assignments
        || (a == 198 && (b == 18 || b == 19))) // benchmarking
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_public_v4(v4);
    }
    let s = ip.segments();
    !(ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_multicast()
        || (s[0] & 0xfe00) == 0xfc00 // unique local
        || (s[0] & 0xffc0) == 0xfe80 // link local
        || (s[0] == 0x2001 && s[1] == 0x0db8) // documentation
        || (s[0] == 0x2001 && s[1] == 0x0000) // Teredo
        || s[0] == 0x2002 // 6to4 embeds an arbitrary v4 address
        || (s[0] == 0x0064 && s[1] == 0xff9b) // NAT64 can reach private v4
        || (s[0] == 0 && s[1] == 0 && s[2] == 0 && s[3] == 0)) // ::/64: v4-compatible/translated
}

pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => is_public_v6(v6),
    }
}

/// Basic URL checks before any network access (also applied to redirects).
pub fn check_url(url: &str) -> Result<(), LogoError> {
    if url.len() > 2048 {
        return Err(LogoError::BadUrl("it's too long"));
    }
    let rest = url
        .strip_prefix("https://")
        .ok_or(LogoError::BadUrl("it must start with https://"))?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    if authority.is_empty() {
        return Err(LogoError::BadUrl("it has no host"));
    }
    if authority.contains('@') || authority.contains('\\') {
        return Err(LogoError::BadUrl(
            "it must not contain a username or password",
        ));
    }
    if let Some((_, port)) = authority.rsplit_once(':') {
        if !authority.ends_with(']') && port != "443" {
            return Err(LogoError::BadUrl("only the standard https port is allowed"));
        }
    }
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(LogoError::BadUrl(
            "it contains spaces or control characters",
        ));
    }
    Ok(())
}

/// Resolve a redirect `Location` against the current URL and apply the same
/// rules as the first request.
fn next_hop(current: &str, location: &str) -> Result<String, LogoError> {
    let base = url::Url::parse(current).map_err(|_| LogoError::Fetch)?;
    let next = base.join(location).map_err(|_| LogoError::Fetch)?;
    if !next.username().is_empty() || next.password().is_some() {
        return Err(LogoError::Fetch);
    }
    let next = next.to_string();
    check_url(&next).map_err(|_| LogoError::Fetch)?;
    Ok(next)
}

/// Resolver that only ever returns public addresses on port 443. ureq dials
/// exactly what this returns, so the checked IP is the one connected to.
struct PublicOnly;

impl ureq::Resolver for PublicOnly {
    fn resolve(&self, netloc: &str) -> std::io::Result<Vec<SocketAddr>> {
        let refuse =
            |why: &str| std::io::Error::new(std::io::ErrorKind::PermissionDenied, why.to_string());
        let addrs: Vec<SocketAddr> = netloc.to_socket_addrs()?.collect();
        if addrs.is_empty() {
            return Err(refuse("no addresses"));
        }
        if addrs.iter().any(|a| a.port() != 443 || !is_public(a.ip())) {
            // Refuse the host entirely if *any* answer is private, so DNS
            // tricks can't mix a public and a private address.
            return Err(refuse("blocked address"));
        }
        Ok(addrs)
    }
}

/// An SVG document: optional BOM/whitespace, optional XML prolog, comments
/// or doctype, then an `<svg` root within the first 1 KB.
fn is_svg(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(1024)];
    let text = match std::str::from_utf8(head) {
        Ok(t) => t,
        Err(e) => match std::str::from_utf8(&head[..e.valid_up_to()]) {
            Ok(t) => t,
            Err(_) => return false,
        },
    };
    let mut t = text.trim_start_matches('\u{feff}').trim_start();
    loop {
        if let Some(rest) = t.strip_prefix("<?") {
            let Some(end) = rest.find("?>") else {
                return false;
            };
            t = rest[end + 2..].trim_start();
        } else if let Some(rest) = t.strip_prefix("<!--") {
            let Some(end) = rest.find("-->") else {
                return false;
            };
            t = rest[end + 3..].trim_start();
        } else if let Some(rest) = t.strip_prefix("<!DOCTYPE") {
            let Some(end) = rest.find('>') else {
                return false;
            };
            t = rest[end + 1..].trim_start();
        } else {
            break;
        }
    }
    t.starts_with("<svg")
}

fn sniff(bytes: &[u8]) -> Option<&'static str> {
    if is_svg(bytes) {
        return Some("image/svg+xml");
    }
    match bytes {
        [0xFF, 0xD8, 0xFF, ..] => Some("image/jpeg"),
        [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, ..] => Some("image/png"),
        [b'G', b'I', b'F', b'8', ..] => Some("image/gif"),
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => Some("image/webp"),
        _ => None,
    }
}

/// Blocking fetch; call from `spawn_blocking`. Returns (content type, bytes).
pub fn fetch(url: &str) -> Result<(&'static str, Vec<u8>), LogoError> {
    check_url(url)?;
    let agent = ureq::AgentBuilder::new()
        .resolver(PublicOnly)
        .redirects(0) // followed below, so every hop is re-checked
        .https_only(true)
        .timeout(Duration::from_secs(5))
        .timeout_connect(Duration::from_secs(3))
        .user_agent("OpenQuote-logo-fetch/1")
        .build();
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut current = url.to_string();
    let mut hops = 0;
    let response = loop {
        if Instant::now() > deadline {
            return Err(LogoError::Fetch);
        }
        let response = match agent.get(&current).set("Accept", "image/*").call() {
            Ok(r) => r,
            Err(ureq::Error::Transport(t)) => {
                let blocked = std::error::Error::source(&t)
                    .and_then(|e| e.downcast_ref::<std::io::Error>())
                    .is_some_and(|e| e.kind() == std::io::ErrorKind::PermissionDenied)
                    || t.to_string().contains("blocked address");
                return Err(if blocked {
                    LogoError::Blocked
                } else {
                    LogoError::Fetch
                });
            }
            Err(_) => return Err(LogoError::Fetch),
        };
        if !(300..400).contains(&response.status()) {
            break response;
        }
        hops += 1;
        if hops > MAX_REDIRECTS {
            return Err(LogoError::Fetch);
        }
        let location = response.header("Location").ok_or(LogoError::Fetch)?;
        current = next_hop(&current, location)?;
    };
    if let Some(len) = response
        .header("Content-Length")
        .and_then(|v| v.parse::<usize>().ok())
    {
        if len > MAX_LOGO_FETCH_BYTES {
            return Err(LogoError::TooLarge);
        }
    }
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(MAX_LOGO_FETCH_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| LogoError::Fetch)?;
    if bytes.len() > MAX_LOGO_FETCH_BYTES {
        return Err(LogoError::TooLarge);
    }
    let kind = sniff(&bytes).ok_or(LogoError::NotImage)?;
    Ok((kind, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn private_and_special_addresses_are_blocked() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.18.0.13",
            "192.168.1.1",
            "169.254.169.254",
            "0.0.0.0",
            "100.64.0.1",
            "192.0.0.1",
            "198.18.0.1",
            "224.0.0.1",
            "255.255.255.255",
            "240.0.0.1",
            "192.0.2.1",
            "::1",
            "::",
            "fc00::1",
            "fd12::1",
            "fe80::1",
            "ff02::1",
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
            "64:ff9b::a00:1",
            "2001:db8::1",
            "::127.0.0.1",
            "2002:7f00:1::1",
            "2001:0:4136:e378::1",
            "::ffff:0:7f00:1",
        ] {
            assert!(!is_public(ip.parse().unwrap()), "{ip} should be blocked");
        }
        for ip in [
            "1.1.1.1",
            "9.9.9.9",
            "8.8.8.8",
            "2606:4700:4700::1111",
            "::ffff:1.1.1.1",
        ] {
            assert!(is_public(ip.parse().unwrap()), "{ip} should be allowed");
        }
    }

    #[test]
    fn url_rules() {
        assert!(check_url("https://example.com/logo.png").is_ok());
        assert!(check_url("https://example.com:443/logo.png").is_ok());
        assert!(check_url("http://example.com/logo.png").is_err());
        assert!(check_url("file:///etc/passwd").is_err());
        assert!(check_url("https://example.com:8080/x.png").is_err());
        assert!(check_url("https://user:pw@example.com/x.png").is_err());
        assert!(check_url("https://example.com\\@evil.test/x.png").is_err());
        assert!(check_url("https:///x.png").is_err());
        assert!(check_url("https://exa mple.com/x.png").is_err());
        assert!(check_url(&format!("https://e.com/{}", "a".repeat(3000))).is_err());
    }

    #[test]
    fn redirect_targets_are_rechecked() {
        let base = "https://example.com/a/logo";
        assert_eq!(
            next_hop(base, "/b.png").unwrap(),
            "https://example.com/b.png"
        );
        assert_eq!(
            next_hop(base, "c.png").unwrap(),
            "https://example.com/a/c.png"
        );
        for bad in [
            "http://example.com/x.png",
            "file:///etc/passwd",
            "data:image/png;base64,AA",
            "mailto:a@b.c",
            "https://user:pw@example.com/x",
            "https://example.com:8080/x",
            "//127.0.0.1:5555/v2/",
        ] {
            assert!(next_hop(base, bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn resolver_refuses_private_targets() {
        use ureq::Resolver;
        assert!(PublicOnly.resolve("127.0.0.1:443").is_err());
        assert!(PublicOnly.resolve("localhost:443").is_err());
        assert!(PublicOnly.resolve("10.0.0.1:443").is_err());
        assert!(PublicOnly.resolve("1.1.1.1:80").is_err()); // wrong port
        assert!(PublicOnly.resolve("1.1.1.1:443").is_ok());
    }

    #[test]
    fn fetch_rejects_local_targets_without_network() {
        assert_eq!(
            fetch("https://127.0.0.1/x.png").unwrap_err(),
            LogoError::Blocked
        );
        assert_eq!(
            fetch("https://localhost/x.png").unwrap_err(),
            LogoError::Blocked
        );
        assert_eq!(
            fetch("https://169.254.169.254/latest").unwrap_err(),
            LogoError::Blocked
        );
        assert!(matches!(
            fetch("http://example.com/x.png").unwrap_err(),
            LogoError::BadUrl(_)
        ));
    }

    #[test]
    fn blocked_and_failed_look_the_same_to_callers() {
        assert_eq!(LogoError::Blocked.message(), LogoError::Fetch.message());
    }

    #[test]
    fn sniffs_image_types_from_bytes() {
        assert_eq!(sniff(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\n...."), Some("image/png"));
        assert_eq!(sniff(b"GIF89a"), Some("image/gif"));
        assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
        assert_eq!(sniff(b"<svg xmlns="), Some("image/svg+xml"));
        assert_eq!(
            sniff(b"\xef\xbb\xbf<?xml version=\"1.0\"?>\n<!-- logo -->\n<!DOCTYPE svg>\n<svg viewBox="),
            Some("image/svg+xml")
        );
        assert_eq!(sniff(b"<html>"), None);
        assert_eq!(sniff(b"<!DOCTYPE html><html><svg>"), None);
    }
}
