//! # Site Architecture & Crawl Depth (Category 12)
//!
//! Evaluates dead-end pages (`WARN_GRAPH_DEAD_END_PAGE`) and excessive crawl depth
//! levels (`WARN_GRAPH_HIGH_CRAWL_DEPTH`).

use crate::core::models::{IssueFinding, PageReport, RuleId};
use crate::graph::SiteGraph;
use crate::rules::catalog::get_rule;

/// Evaluates site architecture health, dead ends, and crawl depth.
pub fn evaluate_architecture(pages: &[PageReport], graph: &SiteGraph) -> Vec<IssueFinding> {
    let mut findings = Vec::new();

    for page in pages {
        // 1. Dead-end page: receives internal inlinks but provides 0 outgoing links
        if graph.in_degree(&page.url) > 0 && graph.out_degree(&page.url) == 0 {
            let rule = get_rule(RuleId::WarnGraphDeadEndPage);
            let msg = format!(
                "Page receives {} internal inlinks but contains zero outgoing links, trapping internal link equity.",
                graph.in_degree(&page.url)
            );
            findings.push(rule.to_finding(&page.url, Some(&msg)));
        }

        // 2. High crawl depth (> 4 clicks from root)
        if page.crawl_depth > 4 {
            let rule = get_rule(RuleId::WarnGraphHighCrawlDepth);
            let msg = format!(
                "Page requires {} link hops to reach from the root seed URL, exceeding the 4-hop depth guideline.",
                page.crawl_depth
            );
            findings.push(rule.to_finding(&page.url, Some(&msg)));
        }
    }

    findings
}
