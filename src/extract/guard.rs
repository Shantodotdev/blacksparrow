//! # Outbound Address Guard
//!
//! Site extraction fetches URLs that come from untrusted input: a URL a user typed, links on
//! scraped pages, stylesheet and subresource addresses. A crawler running inside a server must
//! never be pointed at that server's own network, so every host is resolved here and refused when
//! any of its addresses is private, loopback, link-local (cloud metadata), shared, documentation,
//! benchmarking, reserved, multicast, or an IPv6 range that embeds such an IPv4 address.
//!
//! [`GuardedResolver`] plugs the same policy into `reqwest`, so a direct connection only ever goes
//! to an address that passed the check (no second DNS lookup between check and connect).

use crate::error::{SeoError, SeoResult};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::Arc;
use url::{Host, Url};

/// Why a URL was refused before any request was sent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuardRefusal {
    /// The scheme is not `http` or `https`, the URL carries credentials, or it has no host.
    Unsupported,
    /// The host name does not resolve.
    Unresolved,
    /// The host resolves to (or is) an address the policy refuses.
    BlockedAddress,
}

impl GuardRefusal {
    /// Stable snake_case label used in JSON output.
    pub fn as_str(self) -> &'static str {
        match self {
            GuardRefusal::Unsupported => "unsupported_url",
            GuardRefusal::Unresolved => "unresolved",
            GuardRefusal::BlockedAddress => "blocked_address",
        }
    }
}

/// The address policy of one extraction run.
#[derive(Debug, Clone, Default)]
pub struct AddressPolicy {
    /// Allows private and loopback targets (local test sites only).
    pub allow_private_network: bool,
    /// Hosts that may resolve to private addresses even when `allow_private_network` is off,
    /// written as `host` (any port) or `host:port`. Link-local addresses stay refused.
    pub allowed_private_hosts: Arc<Vec<String>>,
}

impl AddressPolicy {
    /// A policy that refuses private networks, or allows all of them.
    pub fn new(allow_private_network: bool) -> Self {
        Self {
            allow_private_network,
            allowed_private_hosts: Arc::default(),
        }
    }

    /// Whether `ip` may be contacted under this policy. Link-local addresses (cloud metadata
    /// services) are refused even when private networks are allowed.
    pub fn permits(&self, ip: IpAddr) -> bool {
        self.permits_for(ip, false)
    }

    /// Whether `host` is on the private-host allowlist. `port` of `None` matches an entry with
    /// any port (used where only the name is known, as in DNS resolution).
    pub fn allows_host(&self, host: &str, port: Option<u16>) -> bool {
        let host = host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_ascii_lowercase();
        self.allowed_private_hosts.iter().any(|entry| {
            let entry = entry.trim().to_ascii_lowercase();
            let (entry_host, entry_port) = split_host_port(&entry);
            entry_host == host
                && match (entry_port, port) {
                    (Some(want), Some(got)) => want == got,
                    _ => true,
                }
        })
    }

    fn permits_for(&self, ip: IpAddr, host_allowed: bool) -> bool {
        if is_link_local(ip) {
            return false;
        }
        host_allowed || self.allow_private_network || is_public_ip(ip)
    }

    /// Resolves `host` (a name or an IP literal, IPv6 with or without brackets) and returns its
    /// socket addresses when every one of them is permitted.
    ///
    /// # Errors
    ///
    /// Returns [`GuardRefusal::Unresolved`] or [`GuardRefusal::BlockedAddress`].
    pub async fn resolve(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, GuardRefusal> {
        let bare = host.trim_start_matches('[').trim_end_matches(']');
        let addrs: Vec<SocketAddr> = match bare.parse::<IpAddr>() {
            Ok(ip) => vec![SocketAddr::new(ip, port)],
            Err(_) => tokio::net::lookup_host((bare, port))
                .await
                .map_err(|_| GuardRefusal::Unresolved)?
                .collect(),
        };
        if addrs.is_empty() {
            return Err(GuardRefusal::Unresolved);
        }
        let ips: Vec<IpAddr> = addrs.iter().map(SocketAddr::ip).collect();
        self.check_ips(&ips, self.allows_host(bare, Some(port)))?;
        Ok(addrs)
    }

    /// Checks a URL's shape and resolves its host, refusing anything the policy forbids.
    ///
    /// # Errors
    ///
    /// Returns the [`GuardRefusal`] that applies.
    pub async fn check_url(&self, url: &Url) -> Result<(), GuardRefusal> {
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(GuardRefusal::Unsupported);
        }
        let port = url.port_or_known_default().unwrap_or(443);
        let host_allowed = url
            .host_str()
            .is_some_and(|host| self.allows_host(host, Some(port)));
        match url.host() {
            None => Err(GuardRefusal::Unsupported),
            Some(Host::Ipv4(ip)) => self.check_ips(&[IpAddr::V4(ip)], host_allowed),
            Some(Host::Ipv6(ip)) => self.check_ips(&[IpAddr::V6(ip)], host_allowed),
            Some(Host::Domain(domain)) => {
                let addrs: Vec<IpAddr> = tokio::net::lookup_host((domain, port))
                    .await
                    .map_err(|_| GuardRefusal::Unresolved)?
                    .map(|addr| addr.ip())
                    .collect();
                if addrs.is_empty() {
                    return Err(GuardRefusal::Unresolved);
                }
                self.check_ips(&addrs, host_allowed)
            }
        }
    }

    fn check_ips(&self, ips: &[IpAddr], host_allowed: bool) -> Result<(), GuardRefusal> {
        // One refused address refuses the host: a name with a public and a private record is
        // a rebinding setup, not a public site.
        if ips.iter().all(|ip| self.permits_for(*ip, host_allowed)) {
            Ok(())
        } else {
            Err(GuardRefusal::BlockedAddress)
        }
    }
}

/// Splits `host:port` (or `[v6]:port`) into its parts; a bare host or IPv6 literal has no port.
fn split_host_port(entry: &str) -> (&str, Option<u16>) {
    if let Some(rest) = entry.strip_prefix('[') {
        if let Some((host, tail)) = rest.split_once(']') {
            let port = tail.strip_prefix(':').and_then(|p| p.parse().ok());
            return (host, port);
        }
    }
    match entry.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => match port.parse() {
            Ok(port) => (host, Some(port)),
            Err(_) => (entry, None),
        },
        _ => (entry, None),
    }
}

fn is_link_local(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_link_local(),
        IpAddr::V6(v6) => {
            (v6.segments()[0] & 0xffc0) == 0xfe80
                || embedded_ipv4(v6).is_some_and(|v4| v4.is_link_local())
        }
    }
}

/// Whether an address is a public unicast address on the internet.
pub fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_public_v4(v4),
        IpAddr::V6(v6) => is_public_v6(v6),
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_unspecified()
        || ip.is_multicast()
        || a == 0
        // 100.64.0.0/10 shared address space (carrier-grade NAT).
        || (a == 100 && (64..=127).contains(&b))
        // 192.0.0.0/24 IETF protocol assignments.
        || (a == 192 && b == 0 && c == 0)
        // 192.88.99.0/24 deprecated 6to4 relay anycast.
        || (a == 192 && b == 88 && c == 99)
        // 198.18.0.0/15 benchmarking.
        || (a == 198 && (b == 18 || b == 19))
        // 240.0.0.0/4 reserved.
        || a >= 240)
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = embedded_ipv4(ip) {
        return is_public_v4(v4);
    }
    let s = ip.segments();
    let first = s[0];
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        // fc00::/7 unique local.
        || (first & 0xfe00) == 0xfc00
        // fe80::/10 link-local and fec0::/10 deprecated site-local.
        || (first & 0xffc0) == 0xfe80
        || (first & 0xffc0) == 0xfec0
        // 64:ff9b:1::/48 local-use NAT64, which may embed any IPv4 address anywhere.
        || (first == 0x64 && s[1] == 0xff9b && s[2] == 1)
        // 2001::/32 Teredo tunnels and 2001:db8::/32 documentation.
        || (first == 0x2001 && (s[1] == 0 || s[1] == 0x0db8))
        // 100::/64 discard-only.
        || (first == 0x100 && s[1..4] == [0, 0, 0]))
}

/// The IPv4 address an IPv6 address carries in a range a gateway may unwrap: IPv4-mapped,
/// IPv4-compatible, well-known NAT64 and 6to4.
fn embedded_ipv4(ip: Ipv6Addr) -> Option<Ipv4Addr> {
    let s = ip.segments();
    let low = Ipv4Addr::new((s[6] >> 8) as u8, s[6] as u8, (s[7] >> 8) as u8, s[7] as u8);
    let mapped_or_compatible = s[0..5] == [0, 0, 0, 0, 0] && (s[5] == 0 || s[5] == 0xffff);
    let nat64 = s[0] == 0x64 && s[1] == 0xff9b && s[2..6] == [0, 0, 0, 0];
    if mapped_or_compatible || nat64 {
        return Some(low);
    }
    if s[0] == 0x2002 {
        return Some(Ipv4Addr::new(
            (s[1] >> 8) as u8,
            s[1] as u8,
            (s[2] >> 8) as u8,
            s[2] as u8,
        ));
    }
    None
}

/// A `reqwest` DNS resolver that drops every address the policy refuses.
#[derive(Debug, Clone)]
pub struct GuardedResolver {
    policy: AddressPolicy,
}

impl GuardedResolver {
    /// Wraps the policy for use with `reqwest::ClientBuilder::dns_resolver`.
    pub fn new(policy: AddressPolicy) -> Self {
        Self { policy }
    }
}

impl Resolve for GuardedResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let policy = self.policy.clone();
        Box::pin(async move {
            let host = name.as_str().to_string();
            let addrs: Vec<SocketAddr> =
                tokio::net::lookup_host((host.as_str(), 0)).await?.collect();
            let host_allowed = policy.allows_host(&host, None);
            if addrs.is_empty()
                || !addrs
                    .iter()
                    .all(|addr| policy.permits_for(addr.ip(), host_allowed))
            {
                let err: Box<dyn std::error::Error + Send + Sync> =
                    Box::new(SeoError::Network(format!("refused address for {host}")));
                return Err(err);
            }
            let iter: Addrs = Box::new(addrs.into_iter());
            Ok(iter)
        })
    }
}

/// Parses an absolute http(s) URL, rejecting everything else.
///
/// # Errors
///
/// Returns [`SeoError::Url`] for malformed or non-http(s) input.
pub fn parse_web_url(raw: &str) -> SeoResult<Url> {
    let trimmed = raw.trim();
    let candidate = if let Some(rest) = trimmed.strip_prefix("//") {
        format!("https://{rest}")
    } else {
        trimmed.to_string()
    };
    let url = Url::parse(&candidate).map_err(|e| SeoError::Url(format!("{raw}: {e}")))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(SeoError::Url(format!("not an http(s) URL: {raw}")));
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuses_private_and_special_ranges() {
        for raw in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "198.18.0.1",
            "224.0.0.1",
            "255.255.255.255",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
            "64:ff9b::a9fe:a9fe",
            "2002:7f00:1::",
            "fec0::1",
            "2001::1",
            "100::1",
            "64:ff9b:1:a9fe:a9fe::",
            "192.88.99.1",
        ] {
            let ip: IpAddr = raw.parse().unwrap();
            assert!(!is_public_ip(ip), "{raw} must be refused");
        }
    }

    #[test]
    fn allows_public_addresses() {
        for raw in [
            "8.8.8.8",
            "1.1.1.1",
            "2606:4700:4700::1111",
            "::ffff:8.8.8.8",
        ] {
            let ip: IpAddr = raw.parse().unwrap();
            assert!(is_public_ip(ip), "{raw} must be allowed");
        }
    }

    #[tokio::test]
    async fn check_url_refuses_literal_private_hosts_and_credentials() {
        let policy = AddressPolicy::new(false);
        let blocked = Url::parse("http://169.254.169.254/latest/meta-data").unwrap();
        assert_eq!(
            policy.check_url(&blocked).await,
            Err(GuardRefusal::BlockedAddress)
        );
        let creds = Url::parse("https://user:pw@example.com/").unwrap();
        assert_eq!(
            policy.check_url(&creds).await,
            Err(GuardRefusal::Unsupported)
        );
        let ftp = Url::parse("ftp://example.com/").unwrap();
        assert_eq!(policy.check_url(&ftp).await, Err(GuardRefusal::Unsupported));
    }

    #[tokio::test]
    async fn private_network_can_be_allowed_for_local_tests() {
        let policy = AddressPolicy::new(true);
        let local = Url::parse("http://127.0.0.1:8080/").unwrap();
        assert_eq!(policy.check_url(&local).await, Ok(()));
        let metadata = Url::parse("http://169.254.169.254/").unwrap();
        assert_eq!(
            policy.check_url(&metadata).await,
            Err(GuardRefusal::BlockedAddress)
        );
    }

    #[tokio::test]
    async fn allowlisted_hosts_reach_only_themselves() {
        let policy = AddressPolicy {
            allow_private_network: false,
            allowed_private_hosts: Arc::new(vec!["127.0.0.1:8080".to_string()]),
        };
        let ok = Url::parse("http://127.0.0.1:8080/").unwrap();
        let other_port = Url::parse("http://127.0.0.1:6379/").unwrap();
        let other_host = Url::parse("http://192.168.1.1/").unwrap();
        let metadata = Url::parse("http://169.254.169.254/").unwrap();
        assert!(policy.check_url(&ok).await.is_ok());
        assert!(policy.check_url(&other_port).await.is_err());
        assert!(policy.check_url(&other_host).await.is_err());
        assert!(policy.check_url(&metadata).await.is_err());
        assert!(policy.allows_host("127.0.0.1", None));
    }

    #[tokio::test]
    async fn resolve_checks_literals_and_names() {
        let policy = AddressPolicy::new(false);
        assert_eq!(
            policy.resolve("[::1]", 80).await,
            Err(GuardRefusal::BlockedAddress)
        );
        assert_eq!(
            policy.resolve("localhost", 80).await,
            Err(GuardRefusal::BlockedAddress)
        );
        let public = policy.resolve("1.1.1.1", 443).await.unwrap();
        assert_eq!(public, vec!["1.1.1.1:443".parse().unwrap()]);
    }

    #[test]
    fn parse_web_url_accepts_protocol_relative() {
        assert_eq!(
            parse_web_url("//cdn.example.com/a.css").unwrap().as_str(),
            "https://cdn.example.com/a.css"
        );
        assert!(parse_web_url("javascript:alert(1)").is_err());
    }
}
