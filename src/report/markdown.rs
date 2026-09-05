//! # Markdown Report Exporter
//!
//! Generates clean GitHub-Flavored Markdown audit reports formatted for executive review
//! and direct ingestion by LLMs and AI coding assistants.

use crate::core::models::Severity;
use crate::crawler::engine::CrawlResult;
use crate::error::{SeoError, SeoResult};
use crate::rules::catalog::get_rule;
use hashbrown::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Exports a complete audit report as a GitHub-Flavored Markdown document.
pub fn export_markdown_report(result: &CrawlResult, output_dir: &Path) -> SeoResult<PathBuf> {
    fs::create_dir_all(output_dir)
        .map_err(|e| SeoError::Internal(format!("Failed to create output directory: {}", e)))?;

    let host = url::Url::parse(&result.target_url)
        .map(|u| u.host_str().unwrap_or("site").replace('.', "_"))
        .unwrap_or_else(|_| "site_audit".to_string());

    let filename = format!("{}_audit.md", host);
    let output_path = output_dir.join(filename);

    let mut md = String::new();

    // 1. Header & Executive Summary
    md.push_str("# SEO LENS AUDIT REPORT\n\n");
    md.push_str(&format!("**Target Website**: `{}`  \n", result.target_url));
    md.push_str(&format!(
        "**Overall Health Score**: **{}/100**  \n",
        result.health_score
    ));
    md.push_str(&format!("**Pages Crawled**: {}  \n", result.pages.len()));
    md.push_str(&format!(
        "**Total Internal Links**: {}  \n",
        result.graph.edge_count()
    ));
    md.push_str(&format!(
        "**Crawl Duration**: {:.2}s  \n\n",
        result.duration.as_secs_f64()
    ));

    // 2. HTTP Status Breakdown
    let mut status_counts: HashMap<u16, usize> = HashMap::new();
    for page in &result.pages {
        *status_counts.entry(page.status_code).or_default() += 1;
    }

    md.push_str("## 1. HTTP Status Code Breakdown\n\n");
    md.push_str("| Status Code | Description | Count | Percentage |\n");
    md.push_str("| :--- | :--- | :--- | :--- |\n");

    let mut sorted_statuses: Vec<_> = status_counts.into_iter().collect();
    sorted_statuses.sort_by_key(|k| k.0);

    for (status, count) in sorted_statuses {
        let pct = if !result.pages.is_empty() {
            (count as f64 / result.pages.len() as f64) * 100.0
        } else {
            0.0
        };
        let desc = match status {
            200 => "OK",
            301 => "Moved Permanently",
            302 => "Found (Temporary Redirect)",
            403 => "Forbidden",
            404 => "Not Found",
            500 => "Internal Server Error",
            503 => "Service Unavailable",
            _ => "Other",
        };
        md.push_str(&format!(
            "| **{}** | {} | {} | {:.1}% |\n",
            status, desc, count, pct
        ));
    }
    md.push('\n');

    // 3. Technical SEO Issues Breakdown
    md.push_str("## 2. Technical SEO Issues Breakdown\n\n");

    if result.issues.is_empty() {
        md.push_str(
            "✅ **Zero technical SEO defects detected! All document and graph checks passed.**\n\n",
        );
    } else {
        // Group issues by RuleId
        let mut issue_groups: HashMap<
            crate::core::models::RuleId,
            Vec<&crate::core::models::IssueFinding>,
        > = HashMap::new();
        for issue in &result.issues {
            issue_groups.entry(issue.code).or_default().push(issue);
        }

        let mut sorted_groups: Vec<_> = issue_groups.into_iter().collect();
        sorted_groups.sort_by_key(|(_, list)| {
            match list.first().map(|i| i.severity).unwrap_or(Severity::Notice) {
                Severity::Critical => 0,
                Severity::Alert => 1,
                Severity::Warning => 2,
                Severity::Notice => 3,
            }
        });

        for (rule_id, findings) in sorted_groups {
            let rule_def = get_rule(rule_id);
            let badge = match rule_def.severity {
                Severity::Critical => "🚨 CRITICAL",
                Severity::Alert => "⚠️ ALERT",
                Severity::Warning => "⚡ WARNING",
                Severity::Notice => "ℹ️ NOTICE",
            };

            md.push_str(&format!(
                "### {} [{}]: {}\n\n",
                badge,
                rule_id.as_str(),
                rule_def.title
            ));
            md.push_str(&format!("**Affected Pages**: {}  \n", findings.len()));
            md.push_str(&format!("**Description**: {}  \n", rule_def.description));
            md.push_str(&format!("**Remediation**: {}  \n\n", rule_def.fix_advice));

            md.push_str("| # | Affected URL | Finding Context |\n");
            md.push_str("| :-: | :--- | :--- |\n");

            for (idx, finding) in findings.iter().take(20).enumerate() {
                let clean_msg = format_table_cell_message(&finding.message);
                md.push_str(&format!(
                    "| {} | [`{}`]({}) | {} |\n",
                    idx + 1,
                    finding.target_url,
                    finding.target_url,
                    clean_msg
                ));
            }
            if findings.len() > 20 {
                md.push_str(&format!(
                    "\n*...and {} additional affected pages (see complete JSON export for full URL manifest).*\n\n",
                    findings.len() - 20
                ));
            } else {
                md.push('\n');
            }
        }
    }

    // 4. Internal Link Equity (Top PageRank Hubs)
    md.push_str("## 3. Internal Link Equity (Top PageRank Pages)\n\n");
    md.push_str("| Rank | URL | PageRank Equity | Inlinks | Outlinks |\n");
    md.push_str("| :--- | :--- | :--- | :--- | :--- |\n");

    let mut ranked_pages: Vec<_> = result
        .pages
        .iter()
        .map(|p| {
            let pr = result.pagerank.get(&p.url_hash).copied().unwrap_or(0.0);
            (p, pr)
        })
        .collect();

    ranked_pages.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    for (rank, (page, pr)) in ranked_pages.iter().take(15).enumerate() {
        md.push_str(&format!(
            "| {} | [`{}`]({}) | {:.6} | {} | {} |\n",
            rank + 1,
            page.url,
            page.url,
            pr,
            result.graph.in_degree(&page.url),
            result.graph.out_degree(&page.url)
        ));
    }
    md.push('\n');

    fs::write(&output_path, md)
        .map_err(|e| SeoError::Internal(format!("Failed to write markdown report: {}", e)))?;

    Ok(output_path)
}

/// Formats a finding message safely for Markdown tables:
/// 1. Converts quoted URLs `"https?://..."` into clickable Markdown links `[`url`](url)`
/// 2. Escapes raw `<tag>` strings into backticked code spans if not already backticked
/// 3. Escapes `|` as `\|` to preserve Markdown table cell boundaries
fn format_table_cell_message(raw_message: &str) -> String {
    let mut result = String::new();
    let mut remaining = raw_message;

    // Linkify referenced URLs inside quotes: "http://..." -> [`http://...`](http://...)
    while let Some(start_quote) = remaining.find("\"http") {
        result.push_str(&remaining[..start_quote]);
        let after_quote = &remaining[start_quote + 1..];
        if let Some(end_quote) = after_quote.find('"') {
            let url = &after_quote[..end_quote];
            result.push_str(&format!("[`{}`]({})", url, url));
            remaining = &after_quote[end_quote + 1..];
        } else {
            result.push('"');
            remaining = after_quote;
        }
    }
    result.push_str(remaining);

    // Escape pipe characters for table safety
    result = result.replace('|', "\\|");

    // Escape unbackticked <h1>, <h2>, <h3>, <title>, <head>, <meta> to prevent markdownlint MD033 and huge text
    for tag in &[
        "<h1>", "</h1>", "<h2>", "</h2>", "<h3>", "</h3>", "<title>", "</title>", "<head>",
        "</head>", "<meta>", "<img>",
    ] {
        let backticked = format!("`{}`", tag);
        if result.contains(tag) && !result.contains(&backticked) {
            result = result.replace(tag, &backticked);
        }
    }

    result
}
