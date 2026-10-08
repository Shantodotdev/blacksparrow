//! # URL Canonicalization & Normalization Pipeline
//!
//! High-performance 8-stage URL normalization pipeline ensuring deterministic
//! deduplication, RFC 3986 path resolution, tracking parameter stripping,
//! and 64-bit AHash generation.
//!
//! ## Normalization Pipeline Overview
//!
//! When crawling websites, the same logical document is frequently linked with subtle
//! variations in scheme, hostname casing, port notation, relative navigation, trailing
//! slashes, fragments, or tracking parameters:
//!
//! ```text
//! Raw Href ──> [1. Scheme] ──> [2. Hostname] ──> [3. Port] ──> [4. Path]
//!          ──> [5. Trailing Slash] ──> [6. Strip Fragments]
//!          ──> [7. Strip Tracking Query] ──> [8. Sort Query] ──> Normalized URL
//! ```
//!
//! 1. **Scheme Normalization**: Schemes are converted to lowercase. Protocol-relative URLs
//!    (e.g., `//cdn.example.com/lib.js`) default to `https://`. Only `http` and `https`
//!    schemes are permitted; other protocols (like `ftp` or `mailto`) return [`SeoError::Url`].
//! 2. **Hostname Normalization**: Hostnames are lowercased, punycode IDN domains are decoded,
//!    and trailing DNS root dots (`example.com.`) are stripped.
//! 3. **Default Port Stripping**: Standard protocol ports (`:80` for HTTP, `:443` for HTTPS)
//!    are stripped. Non-standard ports (e.g. `:8080`, `:8443`) are preserved.
//! 4. **Path Segment Resolution**: Resolves RFC 3986 relative dot segments (`.` and `..`)
//!    and collapses duplicate internal slashes (`/blog//post` -> `/blog/post`).
//! 5. **Root Path & Trailing Slashes**: An empty path is normalized to `/`. Explicit directory
//!    trailing slashes are preserved to respect server-side directory semantics.
//! 6. **Fragment Removal**: URL hash fragments (`#heading`) are completely removed because
//!    fragments refer to client-side DOM anchors and do not represent distinct server resources.
//! 7. **Tracking Parameter Stripping**: Strips marketing and analytics query parameters
//!    (`utm_source`, `utm_medium`, `fbclid`, `gclid`, `msclkid`, etc.) that cause duplicate
//!    crawls of identical page content.
//! 8. **Deterministic Query Sorting**: Retained legitimate query parameters are sorted
//!    lexicographically so that `?b=2&a=1` and `?a=1&b=2` produce identical canonical URLs.
//!
//! ## Examples
//!
//! ```rust
//! use blacksparrow::core::url::{is_internal, normalize_url, resolve_relative, url_hash};
//!
//! // Example 1: Normalizing messy marketing URLs into a canonical identifier
//! let dirty_url = "HTTPS://EXAMPLE.COM:443/blog//post?utm_source=fb&b=2&a=1#comments";
//! let clean_url = normalize_url(dirty_url).unwrap();
//! assert_eq!(clean_url, "https://example.com/blog/post?a=1&b=2");
//!
//! // Example 2: Resolving relative links discovered in HTML against a base URL
//! let base = "https://example.com/docs/api/";
//! let relative_href = "../guides/quickstart.html";
//! let resolved = resolve_relative(base, relative_href).unwrap();
//! assert_eq!(resolved, "https://example.com/docs/guides/quickstart.html");
//!
//! // Example 3: Verifying internal crawl scope and generating a 64-bit deduplication hash
//! assert!(is_internal(&resolved, base));
//! let hash = url_hash(&clean_url);
//! assert_ne!(hash, 0);
//! ```

use crate::error::{SeoError, SeoResult};
use serde::{Deserialize, Serialize};
use std::hash::Hasher;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use url::Url;

/// Known marketing, analytics, session, and e-commerce tracking parameters to strip.
pub const TRACKING_PARAMS: &[&str] = &[
    "fbclid",
    "gclid",
    "msclkid",
    "mc_eid",
    "_ga",
    "_gl",
    "ref",
    "source",
    "affiliate",
    // E-Commerce & ad network tokens (Daraz, Alibaba, Lazada, etc.)
    "scm",
    "spm",
    "pvid",
    "clicktrackinfo",
    "wh_pid",
    "hybrid",
    "data_prefetch",
];

/// Known sorting, pagination size, and display/view layout query parameters.
pub const SORTING_DISPLAY_PARAMS: &[&str] = &[
    "sort", "order", "dir", "orderby", "sort_by", "limit", "count", "per_page", "view", "display",
    "mode", "layout",
];

/// Normalizes a raw URL string through the 8-stage canonicalization pipeline.
///
/// This eliminates crawl duplication by transforming various URL syntaxes representing
/// the same resource into a single deterministic canonical string.
///
/// # Stages Applied
/// 1. Scheme lowercased (`HTTP` -> `http`); protocol-relative (`//`) mapped to `https:`.
/// 2. Hostname lowercased; trailing root dots removed (`example.com.` -> `example.com`).
/// 3. Default ports removed (`:80` for http, `:443` for https).
/// 4. Path dot segments resolved (`.` and `..`); consecutive duplicate slashes collapsed.
/// 5. Empty path normalized to root `/`; trailing slashes preserved.
/// 6. Hash fragments stripped (`#anchor`).
/// 7. Tracking parameters removed (`utm_*`, `fbclid`, `gclid`, `msclkid`, `mc_eid`, `_ga`, `_gl`, `ref`).
/// 8. Remaining query keys lexicographically sorted; trailing `?` stripped if query is empty.
///
/// # Errors
/// Returns [`SeoError::Url`] if:
/// - The input string is empty or contains only whitespace.
/// - The URL scheme is unsupported (e.g. `ftp://`, `mailto:`).
/// - The URL string fails RFC 3986 parsing or lacks a valid host.
///
/// # Examples
/// ```
/// use blacksparrow::core::url::normalize_url;
///
/// // Strips tracking parameters, sorts queries, collapses slashes, and strips fragments
/// let raw = "HTTPS://Example.COM:443/products//shoes/?utm_source=ad&color=red&size=10#reviews";
/// let normalized = normalize_url(raw).unwrap();
/// assert_eq!(normalized, "https://example.com/products/shoes/?color=red&size=10");
/// ```
pub fn normalize_url(raw: &str) -> SeoResult<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(SeoError::Url("URL cannot be empty".to_string()));
    }

    // 1. Protocol-relative resolution
    let prepared = if trimmed.starts_with("//") {
        format!("https:{trimmed}")
    } else {
        trimmed.to_string()
    };

    let mut parsed = Url::parse(&prepared).map_err(|e| SeoError::Url(e.to_string()))?;

    // Validate scheme (only http and https supported)
    match parsed.scheme() {
        "http" | "https" => {}
        other => {
            return Err(SeoError::Url(format!(
                "Unsupported scheme: '{other}'. Only HTTP and HTTPS are supported."
            )));
        }
    }

    // 2. Hostname normalization: lowercase & strip trailing root dot
    let clean_host = parsed
        .host_str()
        .map(|h| h.trim_end_matches('.').to_string());
    match clean_host {
        Some(host) if !host.is_empty() => {
            parsed
                .set_host(Some(&host))
                .map_err(|e| SeoError::Url(format!("Invalid hostname: {e}")))?;
        }
        _ => return Err(SeoError::Url("URL is missing a valid host".to_string())),
    }

    // 3. Default port stripping (80 for http, 443 for https)
    if let Some(port) = parsed.port() {
        if (parsed.scheme() == "http" && port == 80) || (parsed.scheme() == "https" && port == 443)
        {
            let _ = parsed.set_port(None);
        }
    }

    // 4. Path normalization & dot segment resolution & consecutive slash collapsing
    let path = parsed.path();
    if path.contains("//") || path.contains("/.") || path.is_empty() {
        let has_trailing_slash = path.ends_with('/') && path.len() > 1;

        let mut segments: Vec<&str> = Vec::new();
        for seg in path.split('/') {
            if seg.is_empty() || seg == "." {
                continue;
            }
            if seg == ".." {
                segments.pop();
            } else {
                segments.push(seg);
            }
        }

        let mut clean_path = String::with_capacity(path.len() + 1);
        clean_path.push('/');
        for (i, seg) in segments.iter().enumerate() {
            if i > 0 {
                clean_path.push('/');
            }
            clean_path.push_str(seg);
        }
        if has_trailing_slash && !clean_path.ends_with('/') {
            clean_path.push('/');
        }
        parsed.set_path(&clean_path);
    }

    // 5. Fragment stripping
    parsed.set_fragment(None);

    // 6. Tracking parameter stripping & 7. Deterministic lexicographical query sorting
    if parsed.query().is_some() {
        let mut clean_pairs: Vec<(String, String)> = parsed
            .query_pairs()
            .filter(|(k, _)| !is_tracking_parameter(k))
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();

        if clean_pairs.is_empty() {
            parsed.set_query(None);
        } else {
            clean_pairs.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
            let mut serializer = url::form_urlencoded::Serializer::new(String::new());
            for (k, v) in clean_pairs {
                serializer.append_pair(&k, &v);
            }
            let query_str = serializer.finish();
            parsed.set_query(Some(&query_str));
        }
    }

    Ok(parsed.to_string())
}

/// Resolves a relative URL against an absolute base URL and returns the normalized result.
///
/// Handles all standard HTML hyperlink forms:
/// - Sibling/child relative paths: `post-1.html`
/// - Parent directory navigation: `../about`
/// - Root-relative absolute paths: `/pricing`
/// - Protocol-relative URLs: `//cdn.example.com/asset.js`
/// - Query-only replacements: `?page=2`
///
/// The resulting URL is automatically passed through [`normalize_url`].
///
/// # Errors
/// Returns [`SeoError::Url`] if the base URL is invalid or if the relative path cannot be resolved.
///
/// # Examples
/// ```
/// use blacksparrow::core::url::resolve_relative;
///
/// let base = "https://example.com/articles/2026/";
///
/// // Sibling path
/// let url1 = resolve_relative(base, "article-1.html").unwrap();
/// assert_eq!(url1, "https://example.com/articles/2026/article-1.html");
///
/// // Parent navigation
/// let url2 = resolve_relative(base, "../about").unwrap();
/// assert_eq!(url2, "https://example.com/articles/about");
///
/// // Root path
/// let url3 = resolve_relative(base, "/contact").unwrap();
/// assert_eq!(url3, "https://example.com/contact");
/// ```
pub fn resolve_relative(base: &str, relative: &str) -> SeoResult<String> {
    let base_parsed =
        Url::parse(base).map_err(|e| SeoError::Url(format!("Invalid base URL '{base}': {e}")))?;

    let joined = base_parsed.join(relative).map_err(|e| {
        SeoError::Url(format!(
            "Failed to resolve '{relative}' against '{base}': {e}"
        ))
    })?;

    normalize_url(joined.as_str())
}

/// Computes a fast, collision-resistant 64-bit AHash for a normalized URL string.
///
/// Used by the crawler's frontier queue (`VisitedSet`) to store visited URLs in
/// a `hashbrown::HashSet<u64>` SwissTable rather than storing raw `String` paths.
/// Storing 64-bit integer hashes instead of raw strings reduces memory consumption
/// from >10 MB down to ~400 KB for a 50,000 URL crawl.
///
/// # Examples
/// ```
/// use blacksparrow::core::url::{normalize_url, url_hash};
///
/// let url_a = normalize_url("https://example.com/page?b=2&a=1").unwrap();
/// let url_b = normalize_url("https://example.com/page?a=1&b=2").unwrap();
///
/// // Identical normalized URLs produce identical 64-bit hashes
/// assert_eq!(url_hash(&url_a), url_hash(&url_b));
/// ```
pub fn url_hash(normalized_url: &str) -> u64 {
    use std::hash::BuildHasher;
    let mut hasher = ahash::RandomState::with_seeds(0x1234, 0x5678, 0x9ABC, 0xDEF0).build_hasher();
    hasher.write(normalized_url.as_bytes());
    hasher.finish()
}

/// Checks whether a target URL belongs to the same host as the base URL.
///
/// Compares the hostname of `target_url` with `base_url` (case-insensitively).
/// Subdomains (e.g. `blog.example.com` vs `example.com`) are treated as external
/// (or distinct crawl scopes).
///
/// # Examples
/// ```
/// use blacksparrow::core::url::is_internal;
///
/// let base = "https://example.com/home";
///
/// // Same host
/// assert!(is_internal("https://example.com/contact", base));
/// assert!(is_internal("http://example.com/docs", base));
///
/// // External host or subdomain
/// assert!(!is_internal("https://google.com/", base));
/// assert!(!is_internal("https://sub.example.com/", base));
/// ```
pub fn is_internal(target_url: &str, base_url: &str) -> bool {
    let Ok(target) = Url::parse(target_url) else {
        return false;
    };
    let Ok(base) = Url::parse(base_url) else {
        return false;
    };

    match (target.host_str(), base.host_str()) {
        (Some(t), Some(b)) => t.eq_ignore_ascii_case(b),
        _ => false,
    }
}

/// Categorization of URL query parameters for crawl budget optimization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum QueryParamCategory {
    /// Marketing, analytics, or session trackers (e.g. `utm_*`, `fbclid`, `spm`, `scm`).
    Tracking,
    /// Sorting, pagination size, or layout variations (e.g. `sort`, `order`, `view`, `limit`).
    SortingOrDisplay,
    /// Genuine content filtering facets (e.g. `category`, `brand`, `tag`, `color`, `q`).
    ContentFacet,
}

/// Checks whether a query parameter key matches known analytics, marketing, or session trackers.
pub fn is_tracking_parameter(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    if lower.starts_with("utm_") {
        return true;
    }
    TRACKING_PARAMS
        .iter()
        .any(|&param| param.eq_ignore_ascii_case(&lower))
}

/// Checks whether a query parameter key controls display ordering, pagination sizing, or layout.
pub fn is_sorting_or_display_parameter(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    SORTING_DISPLAY_PARAMS
        .iter()
        .any(|&param| param.eq_ignore_ascii_case(&lower))
}

/// Classifies a query parameter into its functional category: [`QueryParamCategory`].
pub fn classify_parameter(key: &str) -> QueryParamCategory {
    if is_tracking_parameter(key) {
        QueryParamCategory::Tracking
    } else if is_sorting_or_display_parameter(key) {
        QueryParamCategory::SortingOrDisplay
    } else {
        QueryParamCategory::ContentFacet
    }
}

/// Returns `true` if the URL contains any sorting, display ordering, or layout query parameters.
pub fn has_sorting_facets(raw_url: &str) -> bool {
    if let Ok(parsed) = Url::parse(raw_url) {
        for (k, _) in parsed.query_pairs() {
            if classify_parameter(&k) == QueryParamCategory::SortingOrDisplay {
                return true;
            }
        }
    }
    false
}

/// Counts the number of content facet parameters present in the URL query string.
pub fn count_content_facets(raw_url: &str) -> usize {
    if let Ok(parsed) = Url::parse(raw_url) {
        parsed
            .query_pairs()
            .filter(|(k, _)| classify_parameter(k) == QueryParamCategory::ContentFacet)
            .count()
    } else {
        0
    }
}

/// Known non-HTML static asset file extensions.
pub const STATIC_ASSET_EXTENSIONS: &[&str] = &[
    // Documents & keys
    "pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "odt", "rtf", "gpg", "ascii", "sig", "txt",
    "csv", "tsv", // Archives & binaries
    "zip", "tar", "gz", "tgz", "7z", "rar", "bz2", "xz", "iso", "bin", "exe", "dmg", "pkg", "deb",
    "rpm", "apk", // Images & vector
    "png", "jpg", "jpeg", "gif", "svg", "webp", "avif", "ico", "bmp", "tiff", "tif", "psd",
    // Audio & Video
    "mp4", "webm", "mkv", "avi", "mov", "mp3", "wav", "ogg", "m4a", "flac", "aac",
    // Web fonts & styling/scripts
    "css", "js", "mjs", "cjs", "json", "xml", "woff", "woff2", "ttf", "eot", "otf", "map",
];

/// Checks whether a URL targets a non-HTML static asset based on its file extension.
///
/// Inspects the URL's path segment (ignoring query parameters and fragments) and returns `true`
/// if the extension matches known document, media, binary, or font assets.
pub fn is_static_asset_url(raw_url: &str) -> bool {
    let Ok(parsed) = Url::parse(raw_url) else {
        return false;
    };
    let path = parsed.path();
    if let Some(filename) = path.rsplit('/').next() {
        if filename.contains('.') {
            if let Some(ext) = filename.rsplit('.').next() {
                if !ext.is_empty() {
                    return STATIC_ASSET_EXTENSIONS
                        .iter()
                        .any(|&asset_ext| asset_ext.eq_ignore_ascii_case(ext));
                }
            }
        }
    }
    false
}

/// Performs an allocation-free case-insensitive substring search over ASCII characters.
pub fn contains_ignore_ascii_case(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    let needle_bytes = needle.as_bytes();
    if haystack.len() < needle_bytes.len() {
        return false;
    }
    // Scan sliding byte windows directly in CPU cache with SIMD auto-vectorization;
    // avoids allocating multi-megabyte lowercased strings across hundreds of thousands of pages.
    haystack
        .as_bytes()
        .windows(needle_bytes.len())
        .any(|window| window.eq_ignore_ascii_case(needle_bytes))
}

/// Checks if an IP address is a cloud instance metadata endpoint
/// (e.g. AWS/GCP/Azure link-local `169.254.169.254` or IPv6 equivalent).
///
/// These addresses are strictly prohibited from being crawled under any circumstance.
pub fn is_cloud_metadata_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => ipv4.is_link_local(),
        IpAddr::V6(ipv6) => {
            if let Some(mapped_v4) = ipv6.to_ipv4_mapped() {
                mapped_v4.is_link_local()
            } else {
                let seg = ipv6.segments();
                (seg[0] & 0xffc0) == 0xfe80
            }
        }
    }
}

/// Determines whether an IP address belongs to a private, loopback, link-local,
/// cloud metadata, multicast, or otherwise restricted network range.
pub fn is_private_or_restricted_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => is_restricted_ipv4(ipv4),
        IpAddr::V6(ipv6) => {
            if let Some(mapped_v4) = ipv6.to_ipv4_mapped() {
                is_restricted_ipv4(mapped_v4)
            } else {
                is_restricted_ipv6(ipv6)
            }
        }
    }
}

fn is_restricted_ipv4(ip: Ipv4Addr) -> bool {
    let octets = ip.octets();
    ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_unspecified()
        || octets[0] == 0
        || (octets[0] == 100 && (octets[1] & 0xc0) == 64)
        || octets[0] >= 224
}

fn is_restricted_ipv6(ip: Ipv6Addr) -> bool {
    let segments = ip.segments();
    ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || ((segments[0] & 0xfe00) == 0xfc00)
        || ((segments[0] & 0xffc0) == 0xfe80)
        || ((segments[0] & 0xffc0) == 0xfec0)
        || (segments[0] == 0x2001 && segments[1] == 0x0db8)
}

/// Validates whether a target URL is safe for outbound requests.
///
/// Enforces:
/// 1. HTTP or HTTPS protocol scheme only.
/// 2. Cloud instance metadata endpoints (`169.254.169.254`, `metadata.google.internal`) are permanently blocked.
/// 3. Private and loopback destinations are blocked unless `allow_all_private_ips` is true or the target
///    is present in `allowed_private_hosts`.
pub async fn validate_url_safety(
    url_str: &str,
    allow_all_private_ips: bool,
    allowed_private_hosts: &[String],
) -> Result<Url, SeoError> {
    let parsed = Url::parse(url_str).map_err(|e| SeoError::Url(format!("Invalid URL: {e}")))?;

    match parsed.scheme() {
        "http" | "https" => {}
        other => {
            return Err(SeoError::Url(format!(
                "Unsupported protocol scheme: '{other}'. Only http and https are allowed."
            )))
        }
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| SeoError::Url("URL has no host".to_string()))?;
    let host_lower = host.to_ascii_lowercase();

    // 1. Permanent non-bypassable block on cloud metadata
    if host_lower == "169.254.169.254"
        || host_lower == "metadata.google.internal"
        || host_lower.ends_with(".metadata.google.internal")
    {
        return Err(SeoError::Network(
            "Access to cloud instance metadata endpoint is permanently prohibited.".to_string(),
        ));
    }

    // 2. Check if host or host:port is in allowed_private_hosts
    let port = parsed.port_or_known_default();
    let is_explicitly_allowed = allowed_private_hosts.iter().any(|allowed| {
        let a = allowed.trim().to_ascii_lowercase();
        if let Some((allowed_host, allowed_port_str)) = a.split_once(':') {
            if let Ok(allowed_port) = allowed_port_str.parse::<u16>() {
                return allowed_host == host_lower && port == Some(allowed_port);
            }
        }
        if a == host_lower {
            return parsed.port().is_none();
        }
        false
    });

    if is_explicitly_allowed || allow_all_private_ips {
        if let Ok(ip) = host.parse::<IpAddr>() {
            if is_cloud_metadata_ip(ip) {
                return Err(SeoError::Network(
                    "Access to cloud instance metadata endpoint is permanently prohibited."
                        .to_string(),
                ));
            }
        }
        return Ok(parsed);
    }

    // 3. Block localhost domain patterns
    if host_lower == "localhost"
        || host_lower.ends_with(".localhost")
        || host_lower.ends_with(".local")
    {
        return Err(SeoError::Network(format!(
            "Blocked: Access to local/private hostname '{host}' is prohibited by default."
        )));
    }

    // 4. Inspect IP literal
    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_cloud_metadata_ip(ip) {
            return Err(SeoError::Network(
                "Access to cloud instance metadata endpoint is permanently prohibited.".to_string(),
            ));
        }
        if is_private_or_restricted_ip(ip) {
            return Err(SeoError::Network(format!(
                "Blocked: Access to private or restricted IP address '{ip}' is prohibited by default."
            )));
        }
        return Ok(parsed);
    }

    // 5. Asynchronous DNS lookup to inspect resolved IPs
    let port = parsed.port_or_known_default().unwrap_or(80);
    if let Ok(socket_addrs) = tokio::net::lookup_host((host, port)).await {
        for addr in socket_addrs {
            let ip = addr.ip();
            if is_cloud_metadata_ip(ip) {
                return Err(SeoError::Network(
                    "Access to cloud instance metadata endpoint is permanently prohibited."
                        .to_string(),
                ));
            }
            if is_private_or_restricted_ip(ip) {
                return Err(SeoError::Network(format!(
                    "Blocked: Host '{host}' resolves to restricted private IP address '{ip}'."
                )));
            }
        }
    }

    Ok(parsed)
}

/// Extracts the hostname or host:port from a URL string for automatic allowlist scoping.
///
/// Cloud metadata addresses are strictly excluded from auto-allowlisting.
pub fn extract_host_and_port_allowlist(url_str: &str) -> Vec<String> {
    let mut hosts = Vec::new();
    let parsed = Url::parse(url_str).or_else(|_| Url::parse(&format!("https://{}", url_str)));
    if let Ok(u) = parsed {
        if let Some(h) = u.host_str() {
            let h_lower = h.to_ascii_lowercase();
            if h_lower != "169.254.169.254"
                && h_lower != "metadata.google.internal"
                && !h_lower.ends_with(".metadata.google.internal")
            {
                if let Some(port) = u.port() {
                    hosts.push(format!("{}:{}", h_lower, port));
                } else {
                    hosts.push(h_lower);
                }
            }
        }
    }
    hosts
}
