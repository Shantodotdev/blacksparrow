//! # Technical SEO Health Score Calculator
//!
//! Calculates an executive 0–100 SEO health rating based on page count,
//! weighted severity of discovered defects, and category penalty ceilings.

use crate::core::models::{IssueCategory, IssueFinding, Severity};
use hashbrown::HashMap;

/// Maximum penalty deduction allowed from any single audit category (35 points).
///
/// Prevents a single repetitive template-level defect (e.g. orphan alerts or missing H1s
/// across 20,000 product pages) from single-handedly collapsing an otherwise healthy site's
/// score to 0/100.
pub const MAX_CATEGORY_DEDUCTION: f64 = 35.0;

/// Computes a normalized 0–100 technical SEO Health Score.
///
/// Weighting formula per defect:
/// - Critical issues: 5.0 penalty points
/// - Alert issues: 2.0 penalty points
/// - Warning issues: 0.5 penalty points
/// - Notice issues: 0.0 penalty points
///
/// Penalties are grouped by [`IssueCategory`], normalized per crawled page,
/// and capped at [`MAX_CATEGORY_DEDUCTION`] (35.0 points) per category.
pub fn calculate_health_score(total_pages: usize, issues: &[IssueFinding]) -> u8 {
    if total_pages == 0 {
        return 100;
    }

    let pages_count = total_pages.max(1) as f64;
    let mut category_penalties: HashMap<IssueCategory, f64> = HashMap::new();

    for issue in issues {
        let weight = match issue.severity {
            Severity::Critical => 5.0,
            Severity::Alert => 2.0,
            Severity::Warning => 0.5,
            Severity::Notice => 0.0,
        };
        if weight > 0.0 {
            *category_penalties.entry(issue.category).or_insert(0.0) += weight;
        }
    }

    let mut total_deduction = 0.0;
    for (_category, cat_penalty) in category_penalties {
        let cat_penalty_per_page = cat_penalty / pages_count;
        let cat_raw_deduction = cat_penalty_per_page * 10.0;
        let cat_deduction = cat_raw_deduction.min(MAX_CATEGORY_DEDUCTION);
        total_deduction += cat_deduction;
    }

    let raw_score = 100.0 - total_deduction;
    raw_score.clamp(0.0, 100.0).round() as u8
}
