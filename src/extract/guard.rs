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
#[derive(Debug, Clone, Copy)]
pub struct AddressPolicy {
    /// Allows private and loopback targets (local test sites only).
    pub allow_private_network: bool,
}

impl AddressPolicy {
    /// Whether `ip` may be contacted under this policy.
    pub fn permits(&self, ip: IpAddr) -> bool {
        self.allow_private_network || is_public_ip(ip)
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
        match url.host() {
            None => Err(GuardRefusal::Unsupported),
            Some(Host::Ipv4(ip)) => self.check_ips(&[IpAddr::V4(ip)]),
            Some(Host::Ipv6(ip)) => self.check_ips(&[IpAddr::V6(ip)]),
            Some(Host::Domain(domain)) => {
                let addrs: Vec<IpAddr> = tokio::net::lookup_host((domain, port))
                    .await
                    .map_err(|_| GuardRefusal::Unresolved)?
                    .map(|addr| addr.ip())
                    .collect();
                if addrs.is_empty() {
                    return Err(GuardRefusal::Unresolved);
                }
                self.check_ips(&addrs)
            }
        }
    }

    fn check_ips(&self, ips: &[IpAddr]) -> Result<(), GuardRefusal> {
        // One refused address refuses the host: a name with a public and a private record is
        // a rebinding setup, not a public site.
        if ips.iter().all(|ip| self.permits(*ip)) {
            Ok(())
        } else {
            Err(GuardRefusal::BlockedAddress)
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
        // 198.18.0.0/15 benchmarking.
        || (a == 198 && (b == 18 || b == 19))
        // 240.0.0.0/4 reserved.
        || a >= 240)
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = embedded_ipv4(ip) {
        return is_public_v4(v4);
    }
    let first = ip.segments()[0];
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        // fc00::/7 unique local.
        || (first & 0xfe00) == 0xfc00
        // fe80::/10 link-local.
        || (first & 0xffc0) == 0xfe80
        // 2001:db8::/32 documentation.
        || (first == 0x2001 && ip.segments()[1] == 0x0db8))
}

/// The IPv4 address an IPv6 address carries in a range a gateway may unwrap: IPv4-mapped,
/// IPv4-compatible, NAT64 (well-known and local-use) and 6to4.
fn embedded_ipv4(ip: Ipv6Addr) -> Option<Ipv4Addr> {
    let s = ip.segments();
    let low = Ipv4Addr::new((s[6] >> 8) as u8, s[6] as u8, (s[7] >> 8) as u8, s[7] as u8);
    let mapped_or_compatible = s[0..5] == [0, 0, 0, 0, 0] && (s[5] == 0 || s[5] == 0xffff);
    let nat64 = (s[0] == 0x64 && s[1] == 0xff9b && s[2..6] == [0, 0, 0, 0])
        || (s[0] == 0x64 && s[1] == 0xff9b && s[2] == 1);
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
        let policy = self.policy;
        Box::pin(async move {
            let host = name.as_str().to_string();
            let addrs: Vec<SocketAddr> =
                tokio::net::lookup_host((host.as_str(), 0)).await?.collect();
            if addrs.is_empty() || !addrs.iter().all(|addr| policy.permits(addr.ip())) {
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
        let policy = AddressPolicy {
            allow_private_network: false,
        };
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
        let policy = AddressPolicy {
            allow_private_network: true,
        };
        let local = Url::parse("http://127.0.0.1:8080/").unwrap();
        assert_eq!(policy.check_url(&local).await, Ok(()));
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
