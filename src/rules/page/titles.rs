//! # Document Title Rules
//!
//! Evaluates `<title>` presence, character length boundaries, and whitespace formatting.

use crate::core::models::IssueFinding;
use crate::parser::ParsedPage;
use crate::rules::catalog::{get_rule, RuleId};

/// Evaluates `<title>` rules for a parsed document.
pub fn check_titles(page: &ParsedPage, url: &str, issues: &mut Vec<IssueFinding>) {
    match &page.title {
        None => {
            let rule = get_rule(RuleId::ErrTitleMissing);
            issues.push(rule.to_finding(url, None));
        }
        Some(raw_title) => {
            let trimmed = raw_title.trim();
            if trimmed.is_empty() {
                let rule = get_rule(RuleId::ErrTitleMissing);
                issues.push(rule.to_finding(url, Some("Document title is empty.")));
                return;
            }

            // Whitespace padding
            if raw_title.starts_with(char::is_whitespace)
                || raw_title.ends_with(char::is_whitespace)
            {
                let rule = get_rule(RuleId::WarnTitleWhitespacePadded);
                issues.push(
                    rule.to_finding(url, Some("Title contains leading or trailing whitespace.")),
                );
            }

            let char_len = trimmed.chars().count();
            if char_len < 30 {
                let rule = get_rule(RuleId::WarnTitleTooShort);
                issues.push(rule.to_finding(
                    url,
                    Some(&format!(
                        "Title length is {char_len} characters (recommended: 30–60 characters)."
                    )),
                ));
            } else if char_len > 60 {
                let rule = get_rule(RuleId::WarnTitleTooLong);
                issues.push(rule.to_finding(
                    url,
                    Some(&format!(
                        "Title length is {char_len} characters (exceeds recommended 60-character SERP limit)."
                    )),
                ));
            }
        }
    }
}
