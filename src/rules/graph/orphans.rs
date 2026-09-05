//! # Orphan Page Detection (Category 12)
//!
//! Evaluates XML sitemap URLs against internal incoming link counts.
//!
//! Pages declared in XML sitemaps that receive 0 incoming internal links from crawled HTML
//! pages are flagged as orphan pages ([`crate::core::models::RuleId::AlertGraphOrphanPage`]).

use crate::core::models::{IssueFinding, PageReport, RuleId};
use crate::graph::SiteGraph;
use crate::rules::catalog::get_rule;
use hashbrown::HashSet;

/// Evaluates orphan page defects across sitemap URLs and crawled pages.
pub fn evaluate_orphans(
    pages: &[PageReport],
    graph: &SiteGraph,
    sitemap_urls: &[String],
) -> Vec<IssueFinding> {
    let mut findings = Vec::new();
    let mut evaluated_urls = HashSet::new();

    // 1. Evaluate explicit sitemap URLs provided from XML sitemap parsing
    for url in sitemap_urls {
        if evaluated_urls.insert(url.as_str()) && graph.in_degree(url) == 0 {
            let rule = get_rule(RuleId::AlertGraphOrphanPage);
            let msg = format!(
                "URL \"{}\" is declared in the XML sitemap but receives 0 incoming internal links from crawled pages.",
                url
            );
            findings.push(rule.to_finding(url, Some(&msg)));
        }
    }

    // 2. Evaluate any crawled pages marked with is_sitemap_url
    for page in pages {
        if page.is_sitemap_url
            && evaluated_urls.insert(&page.url)
            && graph.in_degree(&page.url) == 0
        {
            let rule = get_rule(RuleId::AlertGraphOrphanPage);
            let msg = format!(
                "URL \"{}\" is tagged as a sitemap URL but receives 0 incoming internal links.",
                page.url
            );
            findings.push(rule.to_finding(&page.url, Some(&msg)));
        }
    }

    findings
}
