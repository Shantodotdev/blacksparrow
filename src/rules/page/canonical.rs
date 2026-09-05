//! # Canonicalization Rules
//!
//! Evaluates canonical tag presence, absolute URL RFC compliance, and URL matching.

use crate::core::models::IssueFinding;
use crate::parser::ParsedPage;
use crate::rules::catalog::{get_rule, RuleId};

/// Evaluates canonical URL rules for a parsed document.
pub fn check_canonical(page: &ParsedPage, url: &str, issues: &mut Vec<IssueFinding>) {
    match &page.canonical_url {
        None => {
            let rule = get_rule(RuleId::WarnCanonicalMissing);
            issues.push(rule.to_finding(url, None));
        }
        Some(canon) => {
            // Check if relative
            if page.is_canonical_relative {
                let rule = get_rule(RuleId::ErrCanonicalRelative);
                issues.push(rule.to_finding(
                    url,
                    Some("Canonical URL declared in HTML is a relative path. RFC specifications require a fully-qualified absolute URL."),
                ));
            }

            let trimmed = canon.trim();
            // Check mismatch
            let clean_canon = trimmed.trim_end_matches('/');
            let clean_url = url.trim_end_matches('/');
            if !clean_canon.eq_ignore_ascii_case(clean_url) {
                let rule = get_rule(RuleId::AlertCanonicalMismatch);
                issues.push(rule.to_finding(
                    url,
                    Some(&format!(
                        "Canonical points to '{trimmed}', which differs from the crawled URL '{url}'."
                    )),
                ));
            }
        }
    }
}
