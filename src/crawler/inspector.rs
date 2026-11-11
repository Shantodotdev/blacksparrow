//! # Single-Page Inspector Engine
//!
//! Performs low-latency single-page fetch, HTML parsing, metadata extraction,
//! and rule evaluation without multi-page crawl frontier overhead.

use crate::core::models::IssueFinding;
use crate::core::url::normalize_url;
use crate::crawler::client::{FetchOptions, FetchResult, HttpClient};
use crate::error::SeoResult;
use crate::parser::{parse_html, ParsedPage};
use crate::rules::evaluate_page;
use std::time::Duration;

/// Fetches and analyzes a single webpage for developer inspection.
///
/// Returns the parsed document metadata, HTTP fetch telemetry, and detected
/// single-page technical SEO defects.
///
/// # Errors
///
/// Returns [`SeoError`] if the URL is invalid or the network request fails.
pub async fn inspect_url(
    url: &str,
    user_agent: &str,
    timeout: Duration,
) -> SeoResult<(ParsedPage, FetchResult, Vec<IssueFinding>)> {
    inspect_url_with_options(url, user_agent, timeout, Vec::new()).await
}

/// Fetches and analyzes a single webpage with custom HTTP request headers.
pub async fn inspect_url_with_options(
    url: &str,
    user_agent: &str,
    timeout: Duration,
    custom_headers: Vec<(String, String)>,
) -> SeoResult<(ParsedPage, FetchResult, Vec<IssueFinding>)> {
    let allowed_hosts = crate::core::url::extract_host_and_port_allowlist(url);
    inspect_url_with_options_ext(
        url,
        user_agent,
        timeout,
        custom_headers,
        false,
        allowed_hosts,
    )
    .await
}

/// Fetches and analyzes a single webpage with custom headers and SSRF network access controls.
pub async fn inspect_url_with_options_ext(
    url: &str,
    user_agent: &str,
    timeout: Duration,
    custom_headers: Vec<(String, String)>,
    allow_all_private_ips: bool,
    mut allowed_private_hosts: Vec<String>,
) -> SeoResult<(ParsedPage, FetchResult, Vec<IssueFinding>)> {
    let normalized = normalize_url(url)?;

    for h in crate::core::url::extract_host_and_port_allowlist(&normalized) {
        if !allowed_private_hosts
            .iter()
            .any(|existing| existing.eq_ignore_ascii_case(&h))
        {
            allowed_private_hosts.push(h);
        }
    }

    let client = HttpClient::new(FetchOptions {
        user_agent: user_agent.to_string(),
        timeout,
        max_redirects: 10,
        custom_headers,
        allow_all_private_ips,
        allowed_private_hosts,
        ..Default::default()
    })?;

    let res = client.fetch(&normalized).await?;

    let is_html = res.content_type.contains("text/html")
        || res.content_type.contains("application/xhtml+xml")
        || res
            .body
            .trim_start()
            .to_ascii_lowercase()
            .starts_with("<!doctype html")
        || res
            .body
            .trim_start()
            .to_ascii_lowercase()
            .starts_with("<html");

    if is_html {
        let parsed = parse_html(&res.body, &res.final_url)?;
        let issues = evaluate_page(&parsed, &res);
        Ok((parsed, res, issues))
    } else {
        let parsed = ParsedPage::default();
        Ok((parsed, res, Vec::new()))
    }
}
