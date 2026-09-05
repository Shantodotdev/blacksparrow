//! # Canonical Loops (Category 12)
//!
//! Detects circular canonical relationships between pages (`ERR_GRAPH_CANONICAL_LOOP`).

use crate::core::models::{IssueFinding, RuleId};
use crate::graph::SiteGraph;
use crate::rules::catalog::get_rule;

/// Evaluates circular canonical loops across the site graph.
pub fn evaluate_canonicals(graph: &SiteGraph) -> Vec<IssueFinding> {
    let mut findings = Vec::new();
    let loops = graph.find_canonical_loops();

    for (url_a, url_b) in loops {
        let rule = get_rule(RuleId::ErrGraphCanonicalLoop);
        let msg = format!(
            "Circular canonical reference detected between \"{}\" and \"{}\".",
            url_a, url_b
        );
        findings.push(rule.to_finding(&url_a, Some(&msg)));
    }

    findings
}
