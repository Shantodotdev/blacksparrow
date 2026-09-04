//! # URL Canonicalization & Normalization Pipeline
//!
//! High-performance 8-stage URL normalization pipeline ensuring deterministic
//! deduplication, RFC 3986 path resolution, tracking parameter stripping,
//! and 64-bit AHash generation.

use crate::error::{SeoError, SeoResult};
use ahash::AHasher;
use std::hash::Hasher;
use url::Url;

/// Known marketing, analytics, and advertising tracking parameters to strip.
const TRACKING_PARAMS: &[&str] = &[
    "utm_source",
    "utm_medium",
    "utm_campaign",
    "utm_term",
    "utm_content",
    "fbclid",
    "gclid",
    "msclkid",
    "mc_eid",
    "_ga",
    "_gl",
    "ref",
];

/// Normalizes a raw URL string through the 8-stage canonicalization pipeline.
///
/// Returns the normalized URL string or an error if the URL is invalid
/// or uses an unsupported scheme.
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
    let clean_host = parsed.host_str().map(|h| h.trim_end_matches('.').to_string());
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
        if (parsed.scheme() == "http" && port == 80)
            || (parsed.scheme() == "https" && port == 443)
        {
            let _ = parsed.set_port(None);
        }
    }

    // 4. Path normalization & dot segment resolution & consecutive slash collapsing
    let path = parsed.path();
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
    clean_path.push_str(&segments.join("/"));
    if has_trailing_slash && !clean_path.ends_with('/') {
        clean_path.push('/');
    }
    parsed.set_path(&clean_path);

    // 5. Fragment stripping
    parsed.set_fragment(None);

    // 6. Tracking parameter stripping & 7. Deterministic lexicographical query sorting
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

    Ok(parsed.to_string())
}

/// Resolves a relative URL against an absolute base URL and returns the normalized result.
pub fn resolve_relative(base: &str, relative: &str) -> SeoResult<String> {
    let base_parsed = Url::parse(base)
        .map_err(|e| SeoError::Url(format!("Invalid base URL '{base}': {e}")))?;

    let joined = base_parsed
        .join(relative)
        .map_err(|e| SeoError::Url(format!("Failed to resolve '{relative}' against '{base}': {e}")))?;

    normalize_url(joined.as_str())
}

/// Computes a fast 64-bit AHash for a normalized URL string.
pub fn url_hash(normalized_url: &str) -> u64 {
    let mut hasher = AHasher::default();
    hasher.write(normalized_url.as_bytes());
    hasher.finish()
}

/// Checks whether a target URL belongs to the same host as the base URL.
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

/// Checks whether a query parameter key matches known analytics or marketing trackers.
fn is_tracking_parameter(key: &str) -> bool {
    TRACKING_PARAMS
        .iter()
        .any(|&param| param.eq_ignore_ascii_case(key))
}
