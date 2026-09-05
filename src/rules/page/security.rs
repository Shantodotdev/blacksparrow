//! # Modern Security & Transport Rules
//!
//! Evaluates HTTPS enforcement, Mixed Content resources, HSTS, CSP, and security headers.

use crate::core::models::IssueFinding;
use crate::crawler::client::FetchResult;
use crate::parser::ParsedPage;
use crate::rules::catalog::{get_rule, RuleId};

/// Evaluates security headers and transport encryption rules.
pub fn check_security(
    page: &ParsedPage,
    fetch: &FetchResult,
    url: &str,
    issues: &mut Vec<IssueFinding>,
) {
    let is_https = url.starts_with("https://");

    // 1. Insecure HTTP
    if !is_https {
        let rule = get_rule(RuleId::ErrSecurityInsecureHttp);
        issues.push(rule.to_finding(url, None));
    } else {
        // 2. Missing HSTS on HTTPS
        if !fetch.headers.contains_key("strict-transport-security") {
            let rule = get_rule(RuleId::WarnSecurityMissingHsts);
            issues.push(rule.to_finding(url, None));
        }

        // 3. Mixed Content Detection (images, scripts, iframes)
        let has_insecure_image = page
            .images
            .iter()
            .any(|img| img.src_url.starts_with("http://"));
        let body_has_insecure_iframe =
            fetch.body.contains("src=\"http://") || fetch.body.contains("src='http://");
        let body_has_insecure_form =
            fetch.body.contains("action=\"http://") || fetch.body.contains("action='http://");

        if has_insecure_image || body_has_insecure_iframe || body_has_insecure_form {
            let rule = get_rule(RuleId::ErrSecurityMixedContent);
            issues.push(rule.to_finding(
                url,
                Some("HTTPS page contains insecure HTTP subresources (images, forms, or iframes)."),
            ));
        }
    }

    // 4. Missing CSP
    if !fetch.headers.contains_key("content-security-policy") {
        let rule = get_rule(RuleId::WarnSecurityMissingCsp);
        issues.push(rule.to_finding(url, None));
    }

    // 5. Missing X-Frame-Options
    if !fetch.headers.contains_key("x-frame-options") {
        let rule = get_rule(RuleId::WarnSecurityMissingXFrameOptions);
        issues.push(rule.to_finding(url, None));
    }

    // 6. Missing X-Content-Type-Options
    if !fetch.headers.contains_key("x-content-type-options") {
        let rule = get_rule(RuleId::WarnSecurityMissingXContentType);
        issues.push(rule.to_finding(url, None));
    }
}
