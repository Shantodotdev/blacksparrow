//! # Robots & Indexing Directives Rules
//!
//! Evaluates `noindex`, `nofollow`, `noarchive`, and `nosnippet` robots flags.

use crate::core::models::{IssueFinding, RobotsFlags};
use crate::parser::ParsedPage;
use crate::rules::catalog::{get_rule, RuleId};

/// Evaluates robots and indexing directive rules.
pub fn check_directives(page: &ParsedPage, url: &str, issues: &mut Vec<IssueFinding>) {
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
}
