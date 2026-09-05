//! # Technical SEO Health Score Calculator
//!
//! Calculates an executive 0–100 SEO health rating based on page count and
//! weighted severity of discovered defects.

use crate::core::models::{IssueFinding, Severity};

/// Computes a normalized 0–100 technical SEO Health Score.
///
/// Weighting formula:
/// - Critical issues: 5.0 penalty points
/// - Alert issues: 2.0 penalty points
/// - Warning issues: 0.5 penalty points
/// - Notice issues: 0.0 penalty points
///
/// Penalties are scaled per crawled page so that large sites aren't penalized
/// for scale while single-page audits maintain accurate defect sensitivity.
pub fn calculate_health_score(total_pages: usize, issues: &[IssueFinding]) -> u8 {
    if total_pages == 0 {
        return 100;
    }

    let mut critical_count = 0usize;
    let mut alert_count = 0usize;
    let mut warning_count = 0usize;

    for issue in issues {
        match issue.severity {
            Severity::Critical => critical_count += 1,
            Severity::Alert => alert_count += 1,
            Severity::Warning => warning_count += 1,
            Severity::Notice => {}
        }
    }

    let total_penalty =
        (critical_count as f64 * 5.0) + (alert_count as f64 * 2.0) + (warning_count as f64 * 0.5);

    let penalty_per_page = total_penalty / (total_pages as f64);
    let raw_score = 100.0 - (penalty_per_page * 10.0);

    raw_score.clamp(0.0, 100.0).round() as u8
}
