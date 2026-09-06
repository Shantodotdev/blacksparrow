//! # Post-Crawl Multi-Page Graph Rules (Phase 2)
//!
//! Evaluates site-wide architectural properties, orphan pages, redirect/canonical loops,
//! duplicate content clusters, dead ends, crawl depth, and reciprocal hreflang validity.

pub mod architecture;
pub mod canonicals;
pub mod duplicates;
pub mod hreflang;
pub mod orphans;
pub mod redirects;

use crate::core::models::{IssueFinding, PageReport};
use crate::graph::SiteGraph;

/// Evaluates all post-crawl multi-page graph rules across the site graph and page collection.
pub fn evaluate_graph_rules(
    pages: &[PageReport],
    graph: &SiteGraph,
    sitemap_urls: &[String],
    crawl_exhaustive: bool,
) -> Vec<IssueFinding> {
    let mut findings = Vec::new();

    findings.extend(orphans::evaluate_orphans(
        pages,
        graph,
        sitemap_urls,
        crawl_exhaustive,
    ));
    findings.extend(redirects::evaluate_redirects(graph));
    findings.extend(canonicals::evaluate_canonicals(graph));
    findings.extend(duplicates::evaluate_duplicates(pages));
    findings.extend(architecture::evaluate_architecture(pages, graph));
    findings.extend(hreflang::evaluate_hreflang(pages));

    findings
}
