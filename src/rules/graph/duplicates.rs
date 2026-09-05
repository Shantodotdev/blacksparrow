//! # Duplicate Content & Metadata Detection (Category 12)
//!
//! Identifies exact duplicate content (content hash collisions), near-duplicate content
//! (SimHash similarity $\ge 85\%$), duplicate document titles, and duplicate meta descriptions.

use crate::core::models::{IssueFinding, PageReport, RuleId};
use crate::parser::content::hamming_distance;
use crate::rules::catalog::get_rule;
use hashbrown::{HashMap, HashSet};

/// Evaluates content and metadata duplication across all crawled pages.
pub fn evaluate_duplicates(pages: &[PageReport]) -> Vec<IssueFinding> {
    let mut findings = Vec::new();

    // 1. Exact Duplicate Content (content_hash)
    let mut hash_groups: HashMap<u64, Vec<&PageReport>> = HashMap::new();
    for page in pages {
        if page.content_hash != 0 && page.word_count >= 50 {
            hash_groups.entry(page.content_hash).or_default().push(page);
        }
    }

    let mut exact_dup_urls = HashSet::new();
    for (_hash, group) in hash_groups {
        if group.len() > 1 {
            let primary_url = &group[0].url;
            for duplicate_page in &group[1..] {
                exact_dup_urls.insert(&duplicate_page.url);
                let rule = get_rule(RuleId::WarnGraphExactDuplicateContent);
                let msg = format!(
                    "Exact duplicate content detected matching primary URL \"{}\".",
                    primary_url
                );
                findings.push(rule.to_finding(&duplicate_page.url, Some(&msg)));
            }
        }
    }

    // 2. Near-Duplicate Content (SimHash Hamming distance <= 3)
    let mut flagged_near_pairs = HashSet::new();
    for i in 0..pages.len() {
        let p1 = &pages[i];
        if p1.simhash == 0 || p1.word_count < 50 || exact_dup_urls.contains(&p1.url) {
            continue;
        }

        for p2 in pages.iter().skip(i + 1) {
            if p2.simhash == 0 || p2.word_count < 50 || exact_dup_urls.contains(&p2.url) {
                continue;
            }

            // Exclude identical content hashes already flagged as exact duplicate
            if p1.content_hash != 0 && p1.content_hash == p2.content_hash {
                continue;
            }

            let dist = hamming_distance(p1.simhash, p2.simhash);
            if dist <= 3 {
                let pair_key = if p1.url < p2.url {
                    (&p1.url, &p2.url)
                } else {
                    (&p2.url, &p1.url)
                };

                if flagged_near_pairs.insert(pair_key) {
                    let similarity_pct = ((64.0 - dist as f64) / 64.0) * 100.0;
                    let rule = get_rule(RuleId::WarnGraphNearDuplicateContent);
                    let msg = format!(
                        "Near-duplicate content ({:.1}% similarity, Hamming distance {}) detected matching \"{}\".",
                        similarity_pct, dist, p1.url
                    );
                    findings.push(rule.to_finding(&p2.url, Some(&msg)));
                }
            }
        }
    }

    // 3. Duplicate Titles
    let mut title_groups: HashMap<&str, Vec<&str>> = HashMap::new();
    for page in pages {
        if let Some(ref title) = page.title {
            let clean = title.trim();
            if !clean.is_empty() {
                title_groups.entry(clean).or_default().push(&page.url);
            }
        }
    }

    for (title, urls) in title_groups {
        if urls.len() > 1 {
            let primary_url = urls[0];
            for &dup_url in &urls[1..] {
                let rule = get_rule(RuleId::WarnGraphDuplicateTitles);
                let msg = format!(
                    "Duplicate title tag \"{}\" shared with \"{}\".",
                    title, primary_url
                );
                findings.push(rule.to_finding(dup_url, Some(&msg)));
            }
        }
    }

    // 4. Duplicate Meta Descriptions
    let mut desc_groups: HashMap<&str, Vec<&str>> = HashMap::new();
    for page in pages {
        if let Some(ref desc) = page.meta_description {
            let clean = desc.trim();
            if !clean.is_empty() {
                desc_groups.entry(clean).or_default().push(&page.url);
            }
        }
    }

    for (desc, urls) in desc_groups {
        if urls.len() > 1 {
            let primary_url = urls[0];
            for &dup_url in &urls[1..] {
                let rule = get_rule(RuleId::WarnGraphDuplicateMetaDescs);
                let msg = format!(
                    "Duplicate meta description shared with \"{}\": \"{}\"",
                    primary_url, desc
                );
                findings.push(rule.to_finding(dup_url, Some(&msg)));
            }
        }
    }

    findings
}
