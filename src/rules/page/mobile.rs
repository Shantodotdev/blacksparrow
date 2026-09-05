//! # Mobile UX & Viewport Rules
//!
//! Evaluates viewport presence and mobile zoom accessibility.

use crate::core::models::IssueFinding;
use crate::parser::ParsedPage;
use crate::rules::catalog::{get_rule, RuleId};

/// Evaluates mobile responsiveness and viewport configuration.
pub fn check_mobile(page: &ParsedPage, url: &str, issues: &mut Vec<IssueFinding>) {
    match &page.viewport {
        None => {
            let rule = get_rule(RuleId::ErrMobileNoViewport);
            issues.push(rule.to_finding(url, None));
        }
        Some(viewport) => {
            let vp_lower = viewport.to_lowercase();
            if vp_lower.contains("user-scalable=no")
                || vp_lower.contains("user-scalable=0")
                || vp_lower.contains("maximum-scale=1.0")
                || vp_lower.contains("maximum-scale=1,")
            {
                let rule = get_rule(RuleId::WarnMobileViewportNonScalable);
                issues.push(rule.to_finding(
                    url,
                    Some("Viewport restricts user scaling/zooming, violating WCAG mobile accessibility."),
                ));
            }
        }
    }
}
