//! # AI Search, GEO & Content Quality Rules
//!
//! Evaluates thin editorial content and placeholder Lorem Ipsum text.

use crate::core::models::IssueFinding;
use crate::crawler::client::FetchResult;
use crate::parser::ParsedPage;
use crate::rules::catalog::{get_rule, RuleId};

/// Evaluates content quality and Generative Engine Optimization (GEO) heuristics.
pub fn check_content_and_ai(
    page: &ParsedPage,
    fetch: &FetchResult,
    url: &str,
    issues: &mut Vec<IssueFinding>,
) {
    // 1. Thin Editorial Content (< 200 words)
    // Only flag on 200 OK responses with HTML content
    if fetch.status_code == 200 && page.word_count < 200 {
        let rule = get_rule(RuleId::WarnContentThin);
        issues.push(rule.to_finding(
            url,
            Some(&format!(
                "Editorial body text contains only {} words (threshold: 200 words).",
                page.word_count
            )),
        ));
    }

    // 2. Placeholder Lorem Ipsum Text
    if fetch.body.to_lowercase().contains("lorem ipsum") {
        let rule = get_rule(RuleId::WarnLoremIpsumDetected);
        issues.push(rule.to_finding(
            url,
            Some("Unfinished placeholder 'Lorem ipsum' dummy text detected in document body."),
        ));
    }
}
