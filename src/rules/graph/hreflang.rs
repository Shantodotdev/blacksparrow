//! # Hreflang Graph Reciprocity & Validity (Category 11)
//!
//! Validates cross-page bidirectional hreflang links, non-canonical targets,
//! and broken/redirecting targets.

use crate::core::models::{IssueFinding, PageReport, RuleId};
use crate::rules::catalog::get_rule;
use hashbrown::HashMap;

/// Evaluates cross-page hreflang configuration across all crawled pages.
pub fn evaluate_hreflang(pages: &[PageReport]) -> Vec<IssueFinding> {
    let mut findings = Vec::new();

    // Map URL to PageReport reference for fast cross-page validation
    let page_map: HashMap<&str, &PageReport> = pages.iter().map(|p| (p.url.as_str(), p)).collect();

    for page in pages {
        for hreflang in &page.hreflangs {
            let target_url = hreflang.target_url.as_str();

            // Self-referencing hreflang tag is valid and not evaluated for reciprocity
            if target_url == page.url.as_str() {
                continue;
            }

            if let Some(&target_page) = page_map.get(target_url) {
                // 1. Check if hreflang points to a broken or redirecting page
                if target_page.status_code >= 300 {
                    let rule = get_rule(RuleId::ErrHreflangToBrokenOrRedirect);
                    let msg = format!(
                        "Hreflang tag for \"{}\" points to \"{}\" which returned HTTP {}.",
                        hreflang.lang_code, target_url, target_page.status_code
                    );
                    findings.push(rule.to_finding(&page.url, Some(&msg)));
                }

                // 2. Check if hreflang points to a non-canonical URL
                if let Some(ref canon) = target_page.canonical_url {
                    if canon != &target_page.url {
                        let rule = get_rule(RuleId::ErrHreflangToNonCanonical);
                        let msg = format!(
                            "Hreflang tag for \"{}\" points to non-canonical URL \"{}\" (authoritative canonical is \"{}\").",
                            hreflang.lang_code, target_url, canon
                        );
                        findings.push(rule.to_finding(&page.url, Some(&msg)));
                    }
                }

                // 3. Check for bidirectional reciprocity
                let has_reciprocal = target_page
                    .hreflangs
                    .iter()
                    .any(|h| h.target_url == page.url);

                if !has_reciprocal {
                    let rule = get_rule(RuleId::ErrHreflangNotReciprocal);
                    let msg = format!(
                        "Hreflang tag for \"{}\" points to \"{}\", but the target page does not reciprocally link back to this URL.",
                        hreflang.lang_code, target_url
                    );
                    findings.push(rule.to_finding(&page.url, Some(&msg)));
                }
            }
        }
    }

    findings
}
