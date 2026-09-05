//! # Image & Core Web Vitals (CLS) Rules
//!
//! Evaluates image `alt` text presence, explicit width/height dimensions, and data URIs.

use crate::core::models::IssueFinding;
use crate::parser::ParsedPage;
use crate::rules::catalog::{get_rule, RuleId};

/// Evaluates image optimization and layout stability rules.
pub fn check_images(page: &ParsedPage, url: &str, issues: &mut Vec<IssueFinding>) {
    let mut missing_alt_count = 0;
    let mut missing_dim_count = 0;
    let mut data_uri_count = 0;

    for img in &page.images {
        // Missing alt attribute
        if img.alt_text.is_none()
            || img
                .alt_text
                .as_deref()
                .map(|s| s.trim().is_empty())
                .unwrap_or(false)
        {
            missing_alt_count += 1;
        }

        // Missing dimensions (CLS risk)
        if !img.has_dimensions {
            missing_dim_count += 1;
        }

        // Inline Data URI
        if img.src_url.starts_with("data:image/") {
            data_uri_count += 1;
        }
    }

    if missing_alt_count > 0 {
        let rule = get_rule(RuleId::WarnImageMissingAlt);
        issues.push(rule.to_finding(
            url,
            Some(&format!(
                "{missing_alt_count} image(s) lack descriptive alt attributes."
            )),
        ));
    }

    if missing_dim_count > 0 {
        let rule = get_rule(RuleId::WarnImageMissingDimensions);
        issues.push(rule.to_finding(
            url,
            Some(&format!(
                "{missing_dim_count} image(s) lack explicit width and height dimensions, triggering CLS."
            )),
        ));
    }

    if data_uri_count > 0 {
        let rule = get_rule(RuleId::WarnImageDataUri);
        issues.push(rule.to_finding(
            url,
            Some(&format!(
                "{data_uri_count} image(s) are embedded inline as base64 data URIs."
            )),
        ));
    }
}
