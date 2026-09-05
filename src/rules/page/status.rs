//! # HTTP Status & Transport Rules
//!
//! Evaluates HTTP status codes (4xx, 5xx, 3xx), WAF bot challenges, and TTFB latency.

use crate::core::models::IssueFinding;
use crate::crawler::client::FetchResult;
use crate::rules::catalog::{get_rule, RuleId};

/// Evaluates network transport and HTTP status rules on a fetch result.
pub fn check_status(fetch: &FetchResult, issues: &mut Vec<IssueFinding>) {
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
}
