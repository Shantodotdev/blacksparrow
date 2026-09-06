//! # Heading Hierarchy & H1 Rules
//!
//! Evaluates H1 presence, multiplicity, empty headings, length, and hierarchy skipping.

use crate::core::models::IssueFinding;
use crate::parser::ParsedPage;
use crate::rules::catalog::{get_rule, RuleId};

/// Evaluates heading hierarchy rules for a parsed document.
pub fn check_headings(page: &ParsedPage, url: &str, issues: &mut Vec<IssueFinding>) {
    // 1. Missing H1
    if page.h1_count == 0 {
        let rule = get_rule(RuleId::ErrH1Missing);
        issues.push(rule.to_finding(url, None));
    } else {
        // 2. Multiple H1 tags
        if page.h1_count > 1 {
            let rule = get_rule(RuleId::WarnH1Multiple);
            issues.push(rule.to_finding(
                url,
                Some(&format!(
                    "Page defines {} `<h1>` tags. Modern SEO recommends a single primary `<h1>`.",
                    page.h1_count
                )),
            ));
        }

        // 3. Primary H1 Content & Length
        if let Some(ref h1) = page.h1_primary {
            let trimmed = h1.trim();
            if trimmed.is_empty() {
                let rule = get_rule(RuleId::WarnH1Empty);
                issues.push(rule.to_finding(
                    url,
                    Some("Primary `<h1>` tag is empty or contains only whitespace."),
                ));
            } else if trimmed.chars().count() > 70 {
                let rule = get_rule(RuleId::WarnH1TooLong);
                issues.push(rule.to_finding(
                    url,
                    Some(&format!(
                        "Primary `<h1>` is {} characters long (recommended: under 70 characters).",
                        trimmed.chars().count()
                    )),
                ));
            }
        }
    }

    // 4. Heading Hierarchy Skipped
    if page.h1_count > 0 && page.h2_headings.is_empty() && !page.h3_headings.is_empty() {
        let rule = get_rule(RuleId::WarnHeadingHierarchySkipped);
        issues.push(rule.to_finding(
            url,
            Some("Document hierarchy skips from `<h1>` directly to `<h3>` with zero `<h2>` headings."),
        ));
    }

    // 5. Duplicate Heading Text
    let mut has_duplicate_heading = false;
    let mut seen_h2 = std::collections::HashSet::new();
    for h2 in &page.h2_headings {
        let trimmed = h2.trim();
        if !trimmed.is_empty() && !seen_h2.insert(trimmed.to_lowercase()) {
            has_duplicate_heading = true;
            break;
        }
    }
    if !has_duplicate_heading {
        let mut seen_h3 = std::collections::HashSet::new();
        for h3 in &page.h3_headings {
            let trimmed = h3.trim();
            if !trimmed.is_empty() && !seen_h3.insert(trimmed.to_lowercase()) {
                has_duplicate_heading = true;
                break;
            }
        }
    }
    if has_duplicate_heading {
        let rule = get_rule(RuleId::WarnDuplicateHeadingText);
        issues.push(rule.to_finding(url, Some("Multiple headings share identical text content.")));
    }

    // 6. Excessive DOM Depth / Element Count (> 1500 elements)
    if page.dom_element_count > 1500 {
        let rule = get_rule(RuleId::WarnExcessiveDomDepth);
        issues.push(rule.to_finding(
            url,
            Some(&format!(
                "Page contains {} DOM elements (threshold: 1500 elements).",
                page.dom_element_count
            )),
        ));
    }
}
