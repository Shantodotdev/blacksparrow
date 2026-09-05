//! # Redirect Loops and Chains (Category 12)
//!
//! Detects circular redirect loops (`ERR_GRAPH_REDIRECT_LOOP`) and multi-hop
//! redirect chains (`WARN_GRAPH_REDIRECT_CHAIN`).

use crate::core::models::{IssueFinding, RuleId};
use crate::graph::SiteGraph;
use crate::rules::catalog::get_rule;

/// Evaluates redirect loops and multi-hop redirect chains across the site graph.
pub fn evaluate_redirects(graph: &SiteGraph) -> Vec<IssueFinding> {
    let mut findings = Vec::new();

    // 1. Check for circular redirect loops
    let loops = graph.find_redirect_loops();
    for cycle in loops {
        if let Some(start_url) = cycle.first() {
            let rule = get_rule(RuleId::ErrGraphRedirectLoop);
            let msg = format!("Circular redirect loop detected: {}", cycle.join(" -> "));
            findings.push(rule.to_finding(start_url, Some(&msg)));
        }
    }

    // 2. Check for multi-hop redirect chains
    let chains = graph.find_redirect_chains();
    for chain in chains {
        if let Some(start_url) = chain.first() {
            let hop_count = chain.len().saturating_sub(1);
            let rule = get_rule(RuleId::WarnGraphRedirectChain);
            let msg = format!(
                "Multi-hop redirect chain ({} hops): {}",
                hop_count,
                chain.join(" -> ")
            );
            findings.push(rule.to_finding(start_url, Some(&msg)));
        }
    }

    findings
}
