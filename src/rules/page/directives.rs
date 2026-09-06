//! # Robots & Indexing Directives Rules
//!
//! Evaluates `noindex`, `nofollow`, `noarchive`, `nosnippet`, pagination canonical/noindex, and SPA heuristics.

use crate::core::models::{IssueFinding, RobotsFlags};
use crate::crawler::client::FetchResult;
use crate::parser::ParsedPage;
use crate::rules::catalog::{get_rule, RuleId};

fn is_pagination_url(url: &str) -> bool {
    let lower = url.to_lowercase();
    if let Ok(parsed) = url::Url::parse(url) {
        if let Some(query) = parsed.query() {
            let q = query.to_lowercase();
            if q.split('&').any(|pair| {
                let key = pair.split('=').next().unwrap_or("");
                key == "page" || key == "p" || key == "pg" || key == "paged"
            }) {
                return true;
            }
        }
        let path = parsed.path().to_lowercase();
        if path.contains("/page/") || path.contains("/p/") {
            return true;
        }
    } else if lower.contains("page=")
        || lower.contains("p=")
        || lower.contains("/page/")
        || lower.contains("/p/")
    {
        return true;
    }
    false
}

/// Evaluates robots and indexing directive rules.
pub fn check_directives(
    page: &ParsedPage,
    fetch: &FetchResult,
    url: &str,
    issues: &mut Vec<IssueFinding>,
) {
    if page.robots_flags.contains(RobotsFlags::NOINDEX) {
        let rule = get_rule(RuleId::AlertIndexingBlockedNoindex);
        issues.push(rule.to_finding(url, None));
    }

    if page.robots_flags.contains(RobotsFlags::NOFOLLOW) {
        let rule = get_rule(RuleId::WarnLinkEquityBlockedNofollow);
        issues.push(rule.to_finding(url, None));
    }

    if page.robots_flags.contains(RobotsFlags::NOARCHIVE) {
        let rule = get_rule(RuleId::WarnNoarchivePresent);
        issues.push(rule.to_finding(url, None));
    }

    if page.robots_flags.contains(RobotsFlags::NOSNIPPET) {
        let rule = get_rule(RuleId::WarnNosnippetPresent);
        issues.push(rule.to_finding(url, None));
    }

    // Pagination Checks
    if is_pagination_url(url) {
        if page.canonical_url.is_none() {
            let rule = get_rule(RuleId::WarnPaginationMissingCanonical);
            issues.push(rule.to_finding(
                url,
                Some("Paginated URL lacks an authoritative self-referencing canonical tag."),
            ));
        }

        if page.robots_flags.contains(RobotsFlags::NOINDEX) {
            let rule = get_rule(RuleId::AlertPaginationNoindex);
            issues.push(rule.to_finding(
                url,
                Some("Paginated component URL specifies noindex, blocking search engines from crawling deep pagination links."),
            ));
        }
    }

    // Unrendered SPA Heuristic
    if page.links.is_empty() && page.word_count < 50 {
        let body_lower = fetch.body.to_lowercase();
        let is_spa_root = body_lower.contains("<div id=\"root\">")
            || body_lower.contains("<div id=\"app\">")
            || body_lower.contains("<div id=\"__next\">")
            || body_lower.contains("id=\"root\"")
            || body_lower.contains("id=\"app\"")
            || body_lower.contains("<noscript>you need javascript")
            || body_lower.contains("enable javascript to run this app");

        if is_spa_root {
            let rule = get_rule(RuleId::AlertUnrenderedSpaHeuristic);
            issues.push(rule.to_finding(
                url,
                Some("Client-side Single Page Application (SPA) container detected with empty DOM elements and 0 hyperlinks."),
            ));
        }
    }
}
