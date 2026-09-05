//! # Meta Description Rules
//!
//! Evaluates `<meta name="description">` presence and character length bounds.

use crate::core::models::IssueFinding;
use crate::parser::ParsedPage;
use crate::rules::catalog::{get_rule, RuleId};

/// Evaluates meta description rules for a parsed document.
pub fn check_descriptions(page: &ParsedPage, url: &str, issues: &mut Vec<IssueFinding>) {
    match &page.meta_description {
        None => {
            let rule = get_rule(RuleId::WarnMetaDescMissing);
            issues.push(rule.to_finding(url, None));
        }
        Some(raw_desc) => {
            let trimmed = raw_desc.trim();
            if trimmed.is_empty() {
                let rule = get_rule(RuleId::WarnMetaDescMissing);
                issues.push(rule.to_finding(url, Some("Meta description tag is empty.")));
                return;
            }

            let char_len = trimmed.chars().count();
            if char_len < 70 {
                let rule = get_rule(RuleId::WarnMetaDescTooShort);
                issues.push(rule.to_finding(
                    url,
                    Some(&format!(
                        "Meta description is {char_len} characters (recommended: 70–160 characters)."
                    )),
                ));
            } else if char_len > 160 {
                let rule = get_rule(RuleId::WarnMetaDescTooLong);
                issues.push(rule.to_finding(
                    url,
                    Some(&format!(
                        "Meta description is {char_len} characters (exceeds recommended 160-character snippet boundary)."
                    )),
                ));
            }
        }
    }
}
