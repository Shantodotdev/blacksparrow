//! # Links & Anchor Text Quality Rules
//!
//! Evaluates hyperlink quantities, suspicious/generic anchor texts, and empty anchors.

use crate::core::models::IssueFinding;
use crate::parser::ParsedPage;
use crate::rules::catalog::{get_rule, RuleId};

/// Evaluates link quality and anchor text rules for a parsed document.
pub fn check_links(page: &ParsedPage, url: &str, issues: &mut Vec<IssueFinding>) {
    // 1. Excessive links on page (> 250)
    if page.links.len() > 250 {
        let rule = get_rule(RuleId::WarnLinksTooManyOnPage);
        issues.push(rule.to_finding(
            url,
            Some(&format!(
                "Page contains {} hyperlinks (threshold: 250 links).",
                page.links.len()
            )),
        ));
    }

    let mut suspicious_count = 0;
    let mut empty_count = 0;

    for link in &page.links {
        let trimmed = link.anchor_text.trim();
        if !link.is_image_link && trimmed.is_empty() {
            empty_count += 1;
        } else {
            let lower = trimmed.to_lowercase();
            if matches!(
                lower.as_str(),
                "click here"
                    | "click this"
                    | "here"
                    | "read more"
                    | "learn more"
                    | "more"
                    | "link"
                    | "this link"
                    | "page"
                    | "website"
                    | "continue"
                    | "details"
            ) {
                suspicious_count += 1;
            }
        }
    }

    // 2. Suspicious/generic anchor text
    if suspicious_count > 0 {
        let rule = get_rule(RuleId::WarnLinkSuspiciousAnchor);
        issues.push(rule.to_finding(
            url,
            Some(&format!(
                "{suspicious_count} hyperlink(s) use generic, non-descriptive anchor text (e.g. 'click here', 'read more')."
            )),
        ));
    }

    // 3. Empty anchor text
    if empty_count > 0 {
        let rule = get_rule(RuleId::WarnLinkEmptyAnchor);
        issues.push(rule.to_finding(
            url,
            Some(&format!(
                "{empty_count} text hyperlink(s) have empty anchor text without descriptive context."
            )),
        ));
    }
}
