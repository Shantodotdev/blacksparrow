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
    let normalized = normalize_url(url)?;

    let client = HttpClient::new(FetchOptions {
        user_agent: user_agent.to_string(),
        timeout,
        max_redirects: 10,
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
