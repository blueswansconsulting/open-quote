//! Small in-memory, per-client rate limiting.
//!
//! Fixed windows per client IP, kept in a bounded map. Good enough for a
//! single instance; it resets on restart and isn't shared between replicas.
//!
//! The client IP is the TCP peer unless `TRUSTED_CLIENT_IP_HEADER` is set
//! (e.g. `cf-connecting-ip` behind Cloudflare, or `x-real-ip` behind a proxy
//! you control). Only set it when every request really comes through that
//! proxy, because clients can send the header themselves.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::{
    extract::{ConnectInfo, FromRequestParts},
    http::{request::Parts, HeaderMap},
};

/// Above this many tracked clients, expired entries are swept; if still full,
/// the oldest window is evicted so memory stays bounded.
const MAX_TRACKED: usize = 10_000;

pub struct Limiter {
    limit: u32,
    window: Duration,
    hits: Mutex<HashMap<IpAddr, (Instant, u32)>>,
}

impl Limiter {
    pub fn new(limit: u32, window: Duration) -> Self {
        Limiter {
            limit,
            window,
            hits: Mutex::new(HashMap::new()),
        }
    }

    /// Record a request. `Err(seconds)` means rejected; retry after that long.
    pub fn check(&self, ip: IpAddr) -> Result<(), u64> {
        self.check_at(ip, Instant::now())
    }

    fn check_at(&self, ip: IpAddr, now: Instant) -> Result<(), u64> {
        let ip = bucket(ip);
        let mut hits = self.hits.lock().unwrap_or_else(|e| e.into_inner());
        if hits.len() >= MAX_TRACKED && !hits.contains_key(&ip) {
            hits.retain(|_, (start, _)| now.duration_since(*start) < self.window);
            if hits.len() >= MAX_TRACKED {
                if let Some(oldest) = hits.iter().min_by_key(|(_, (s, _))| *s).map(|(k, _)| *k) {
                    hits.remove(&oldest);
                }
            }
        }
        let entry = hits.entry(ip).or_insert((now, 0));
        if now.duration_since(entry.0) >= self.window {
            *entry = (now, 0);
        }
        if entry.1 >= self.limit {
            let left = self.window.saturating_sub(now.duration_since(entry.0));
            return Err(left.as_secs().max(1));
        }
        entry.1 += 1;
        Ok(())
    }
}

/// One bucket per IPv4 address, and per /64 for IPv6, since a single
/// household or phone normally controls a whole /64.
fn bucket(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
            Some(v4) => IpAddr::V4(v4),
            None => {
                let s = v6.segments();
                IpAddr::from([s[0], s[1], s[2], s[3], 0, 0, 0, 0])
            }
        },
        v4 => v4,
    }
}

/// The client's IP address, for rate limiting.
pub struct ClientIp(pub IpAddr);

pub fn trusted_header() -> Option<String> {
    std::env::var("TRUSTED_CLIENT_IP_HEADER")
        .ok()
        .map(|h| h.trim().to_ascii_lowercase())
        .filter(|h| !h.is_empty())
}

pub fn client_ip(headers: &HeaderMap, peer: Option<IpAddr>, trusted: Option<&str>) -> IpAddr {
    if let Some(name) = trusted {
        let from_header = headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            // X-Forwarded-For style lists: the first entry is the client.
            .and_then(|v| v.split(',').next())
            .and_then(|v| v.trim().parse::<IpAddr>().ok());
        if let Some(ip) = from_header {
            return ip;
        }
    }
    peer.unwrap_or(IpAddr::from([0, 0, 0, 0]))
}

impl<S: Send + Sync> FromRequestParts<S> for ClientIp {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {
        let peer = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|c| c.0.ip());
        Ok(ClientIp(client_ip(
            &parts.headers,
            peer,
            trusted_header().as_deref(),
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn allows_up_to_the_limit_then_rejects_until_the_window_ends() {
        let l = Limiter::new(3, Duration::from_secs(60));
        let t0 = Instant::now();
        for _ in 0..3 {
            assert!(l.check_at(ip("1.2.3.4"), t0).is_ok());
        }
        let wait = l
            .check_at(ip("1.2.3.4"), t0 + Duration::from_secs(10))
            .unwrap_err();
        assert_eq!(wait, 50);
        // Other clients are unaffected.
        assert!(l.check_at(ip("5.6.7.8"), t0).is_ok());
        // A new window resets the count.
        assert!(l
            .check_at(ip("1.2.3.4"), t0 + Duration::from_secs(61))
            .is_ok());
    }

    #[test]
    fn ipv6_clients_share_a_bucket_per_64() {
        let l = Limiter::new(1, Duration::from_secs(60));
        let t0 = Instant::now();
        assert!(l.check_at(ip("2001:db8:1:2::1"), t0).is_ok());
        assert!(l.check_at(ip("2001:db8:1:2:ffff::9"), t0).is_err()); // same /64
        assert!(l.check_at(ip("2001:db8:1:3::1"), t0).is_ok()); // different /64
        assert!(l.check_at(ip("::ffff:203.0.113.5"), t0).is_ok());
        assert!(l.check_at(ip("203.0.113.5"), t0).is_err()); // mapped = same v4
    }

    #[test]
    fn memory_stays_bounded() {
        let l = Limiter::new(1, Duration::from_secs(60));
        let t0 = Instant::now();
        for i in 0..(MAX_TRACKED as u32 + 50) {
            let _ = l.check_at(IpAddr::from(i.to_be_bytes()), t0);
        }
        assert!(l.hits.lock().unwrap().len() <= MAX_TRACKED);
    }

    #[test]
    fn client_ip_uses_trusted_header_only_when_configured() {
        let mut h = HeaderMap::new();
        h.insert("cf-connecting-ip", "203.0.113.9".parse().unwrap());
        h.insert("x-forwarded-for", "198.51.100.7, 10.0.0.1".parse().unwrap());
        let peer = Some(ip("172.18.0.2"));
        assert_eq!(client_ip(&h, peer, None), ip("172.18.0.2"));
        assert_eq!(
            client_ip(&h, peer, Some("cf-connecting-ip")),
            ip("203.0.113.9")
        );
        assert_eq!(
            client_ip(&h, peer, Some("x-forwarded-for")),
            ip("198.51.100.7")
        );
        // Missing or junk header falls back to the peer.
        assert_eq!(client_ip(&h, peer, Some("x-real-ip")), ip("172.18.0.2"));
        h.insert("x-real-ip", "not-an-ip".parse().unwrap());
        assert_eq!(client_ip(&h, peer, Some("x-real-ip")), ip("172.18.0.2"));
    }
}
