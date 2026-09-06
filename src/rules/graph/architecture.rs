//! # Site Architecture & Crawl Depth (Category 12)
//!
//! Evaluates dead-end pages (`WARN_GRAPH_DEAD_END_PAGE`), excessive crawl depth
//! levels (`WARN_GRAPH_HIGH_CRAWL_DEPTH`), and low-equity navigation hubs (`WARN_LOW_INTERNAL_PAGERANK_HUB`).

use crate::core::models::{IssueFinding, PageReport, RuleId};
use crate::core::url::url_hash;
use crate::graph::SiteGraph;
use crate::rules::catalog::get_rule;
use hashbrown::HashMap;

/// Evaluates site architecture health, dead ends, crawl depth, and hub PageRank equity.
pub fn evaluate_architecture(
    pages: &[PageReport],
    graph: &SiteGraph,
    pagerank: &HashMap<u64, f64>,
) -> Vec<IssueFinding> {
    let mut findings = Vec::new();

    for page in pages {
        let out_deg = graph.out_degree(&page.url);
        let in_deg = graph.in_degree(&page.url);

        // 1. Dead-end page: receives internal inlinks but provides 0 outgoing links
        if in_deg > 0 && out_deg == 0 {
            let rule = get_rule(RuleId::WarnGraphDeadEndPage);
            let msg = format!(
                "Page receives {} internal inlinks but contains zero outgoing links, trapping internal link equity.",
                in_deg
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

        // 3. Low Internal PageRank Hub (high outlinks, but receives minimal equity / isolated)
        if out_deg >= 50 {
            let hash = url_hash(&page.url);
            let pr = pagerank.get(&hash).copied().unwrap_or(0.0);
            if pr < 0.0001 || in_deg <= 1 {
                let rule = get_rule(RuleId::WarnLowInternalPagerankHub);
                let msg = format!(
                    "Page serves as a high-volume navigation hub with {} outgoing internal links, but receives low PageRank equity ({:.6}) and only {} incoming links.",
                    out_deg, pr, in_deg
                );
                findings.push(rule.to_finding(&page.url, Some(&msg)));
            }
        }
    }

    findings
}
