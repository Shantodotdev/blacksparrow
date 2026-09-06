//! # HTTP Status & Transport Rules
//!
//! Evaluates HTTP status codes (4xx, 5xx, 3xx), WAF bot challenges, and TTFB latency.

use crate::core::models::IssueFinding;
use crate::crawler::client::FetchResult;
use crate::parser::ParsedPage;
use crate::rules::catalog::{get_rule, RuleId};

/// Evaluates network transport and HTTP status rules on a fetch result.
pub fn check_status(page: &ParsedPage, fetch: &FetchResult, issues: &mut Vec<IssueFinding>) {
    let url = &fetch.final_url;

    // 1. HTTP 4xx Client Errors
    if (400..=499).contains(&fetch.status_code) {
        let rule = get_rule(RuleId::ErrHttp4xxClientError);
        issues.push(rule.to_finding(
            url,
            Some(&format!(
                "HTTP status {} returned for request URL: {}",
                fetch.status_code, fetch.url
            )),
        ));
    }

    // 2. HTTP 5xx Server Errors
    if (500..=599).contains(&fetch.status_code) {
        let rule = get_rule(RuleId::ErrHttp5xxServerError);
        issues.push(rule.to_finding(
            url,
            Some(&format!(
                "Internal server error {} encountered on: {}",
                fetch.status_code, fetch.url
            )),
        ));
    }

    // 3. HTTP 301 Permanent Redirect
    if fetch.status_code == 301 {
        let rule = get_rule(RuleId::InfoHttp301PermanentRedirect);
        issues.push(rule.to_finding(
            url,
            Some(&format!(
                "301 Permanent Redirect from {} to {}",
                fetch.url, fetch.final_url
            )),
        ));
    }

    // 4. HTTP 302 Temporary Redirect
    if fetch.status_code == 302 {
        let rule = get_rule(RuleId::InfoHttp302TemporaryRedirect);
        issues.push(rule.to_finding(
            url,
            Some(&format!(
                "302 Temporary Redirect from {} to {}",
                fetch.url, fetch.final_url
            )),
        ));
    }

    // 5. HTTP 307 / 308 Redirects
    if fetch.status_code == 307 || fetch.status_code == 308 {
        let rule = get_rule(RuleId::InfoHttp307_308Redirect);
        issues.push(rule.to_finding(
            url,
            Some(&format!(
                "{} Redirect from {} to {}",
                fetch.status_code, fetch.url, fetch.final_url
            )),
        ));
    }

    // 6. WAF Challenge Screen
    if let Some(waf) = fetch.waf_detected {
        let rule = get_rule(RuleId::AlertWafBotChallenge);
        issues.push(rule.to_finding(
            url,
            Some(&format!(
                "Bot challenge screen detected by security provider: {waf}"
            )),
        ));
    }

    // 7. Slow TTFB (> 1800ms)
    if fetch.ttfb_ms > 1800 {
        let rule = get_rule(RuleId::WarnSlowTtfb);
        issues.push(rule.to_finding(
            url,
            Some(&format!(
                "Server response latency (TTFB) was {} ms (threshold: 1800 ms)",
                fetch.ttfb_ms
            )),
        ));
    }

    // 8. Faceted Spider Trap (> 2 content facet query parameters)
    let content_facet_count = crate::core::url::count_content_facets(url);
    if content_facet_count > 2 {
        let rule = get_rule(RuleId::AlertFacetedSpiderTrap);
        issues.push(rule.to_finding(
            url,
            Some(&format!(
                "URL contains {} faceted filter parameters, risking a search engine spider trap: {}",
                content_facet_count, url
            )),
        ));
    }

    // 9. Soft 404 Detection
    if fetch.status_code == 200 {
        let is_soft_404_title = page
            .title
            .as_deref()
            .map(|t| {
                let l = t.to_lowercase();
                l.contains("404 not found")
                    || l.contains("page not found")
                    || l.contains("error 404")
            })
            .unwrap_or(false);
        let is_soft_404_h1 = page
            .h1_primary
            .as_deref()
            .map(|h| {
                let l = h.to_lowercase();
                l.contains("404 not found")
                    || l.contains("page not found")
                    || l.contains("error 404")
            })
            .unwrap_or(false);
        let body_lower = fetch.body.to_lowercase();
        let body_has_404_msg = body_lower.contains("404 not found")
            || body_lower.contains("page not found")
            || body_lower.contains("page cannot be found")
            || body_lower.contains("page was not found");

        if (is_soft_404_title || is_soft_404_h1) && (page.word_count < 150 || body_has_404_msg) {
            let rule = get_rule(RuleId::ErrHttpSoft404);
            issues.push(rule.to_finding(
                url,
                Some("Page returned HTTP 200 OK but displays 404 Not Found error messaging."),
            ));
        }
    }

    // 10. Performance HTML payload size
    if fetch.size_bytes > 3_000_000 {
        let rule = get_rule(RuleId::ErrPerfExcessiveHtmlPayload);
        issues.push(rule.to_finding(
            url,
            Some(&format!(
                "HTML document payload size is {:.2} MB (threshold: 3.0 MB).",
                fetch.size_bytes as f64 / 1_000_000.0
            )),
        ));
    } else if fetch.size_bytes > 1_500_000 {
        let rule = get_rule(RuleId::WarnPerfLargeHtmlPayload);
        issues.push(rule.to_finding(
            url,
            Some(&format!(
                "HTML document payload size is {:.2} MB (threshold: 1.5 MB).",
                fetch.size_bytes as f64 / 1_000_000.0
            )),
        ));
    }
}
