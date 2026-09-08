//! # LLM-Optimized Markdown Report Generator
//!
//! Generates dense, high-signal Markdown audit reports engineered specifically for
//! AI coding agents (Claude, Cursor, Windsurf, Antigravity) and LLM context windows:
//! - Strictly ZERO ANSI color escape sequences (`\x1b[`).
//! - Strictly ZERO decorative ASCII art banners (`██`).
//! - Bounded URL sample listings (max 3–5 per rule) to conserve context tokens.
//! - Categorized issues with rule codes, problem statement, and concrete `Action for Agent` instructions.

use crate::core::models::{CrawlSummary, IssueFinding, RuleId, Severity};
use crate::rules::catalog::get_rule;
use hashbrown::HashMap;

/// Formats a complete technical SEO audit into a token-efficient Markdown document for AI agents.
pub fn format_llm_markdown_report(
    summary: &CrawlSummary,
    issues: &[IssueFinding],
    top_issues_limit: usize,
    include_urls: bool,
) -> String {
    let mut md = String::with_capacity(4096);

    let host = url::Url::parse(&summary.target_url)
        .map(|u| u.host_str().unwrap_or("site").to_string())
        .unwrap_or_else(|_| summary.target_url.clone());

    // 1. Executive Summary Header
    md.push_str(&format!("# Technical SEO Audit: {}\n", host));
    md.push_str(&format!(
        "**Health Score**: {}/100 | **Pages Crawled**: {}\n",
        summary.health_score, summary.total_pages_crawled
    ));

    let mut critical_count = 0;
    let mut alert_count = 0;
    let mut warning_count = 0;
    let mut notice_count = 0;

    for issue in issues {
        match issue.severity {
            Severity::Critical => critical_count += 1,
            Severity::Alert => alert_count += 1,
            Severity::Warning => warning_count += 1,
            Severity::Notice => notice_count += 1,
        }
    }

    md.push_str(&format!(
        "**Summary**: {} Critical Issues, {} Alerts, {} Warnings, {} Notices\n\n---\n\n",
        critical_count, alert_count, warning_count, notice_count
    ));

    if issues.is_empty() {
        md.push_str("✅ **Zero technical SEO defects detected! All document, indexability, and graph checks passed.**\n");
        return md;
    }

    // Group issues by RuleId
    let mut grouped_issues: HashMap<RuleId, Vec<&IssueFinding>> = HashMap::new();
    for issue in issues {
        grouped_issues.entry(issue.code).or_default().push(issue);
    }

    let mut sorted_groups: Vec<_> = grouped_issues.into_iter().collect();
    sorted_groups.sort_by_key(|(_, list)| {
        match list.first().map(|i| i.severity).unwrap_or(Severity::Notice) {
            Severity::Critical => 0,
            Severity::Alert => 1,
            Severity::Warning => 2,
            Severity::Notice => 3,
        }
    });

    let limit = if top_issues_limit == 0 {
        20
    } else {
        top_issues_limit
    };

    let mut critical_groups = Vec::new();
    let mut alert_groups = Vec::new();
    let mut warning_groups = Vec::new();
    let mut notice_groups = Vec::new();

    for (rule_id, findings) in sorted_groups.into_iter().take(limit) {
        let sev = findings
            .first()
            .map(|i| i.severity)
            .unwrap_or(Severity::Notice);
        match sev {
            Severity::Critical => critical_groups.push((rule_id, findings)),
            Severity::Alert => alert_groups.push((rule_id, findings)),
            Severity::Warning => warning_groups.push((rule_id, findings)),
            Severity::Notice => notice_groups.push((rule_id, findings)),
        }
    }

    // 2. Critical Issues Section
    if !critical_groups.is_empty() {
        md.push_str("## 🚨 Critical Issues (Immediate Fix Required)\n\n");
        for (idx, (rule_id, findings)) in critical_groups.iter().enumerate() {
            append_issue_block(&mut md, idx + 1, *rule_id, findings, include_urls);
        }
        md.push_str("---\n\n");
    }

    // 3. High-Priority Alerts Section
    if !alert_groups.is_empty() {
        md.push_str("## ⚠️ High-Priority Alerts\n\n");
        for (idx, (rule_id, findings)) in alert_groups.iter().enumerate() {
            append_issue_block(&mut md, idx + 1, *rule_id, findings, include_urls);
        }
        md.push_str("---\n\n");
    }

    // 4. Warnings Section
    if !warning_groups.is_empty() {
        md.push_str("## ⚡ Warnings\n\n");
        for (idx, (rule_id, findings)) in warning_groups.iter().enumerate() {
            append_issue_block(&mut md, idx + 1, *rule_id, findings, include_urls);
        }
        md.push_str("---\n\n");
    }

    // 5. Notices Section
    if !notice_groups.is_empty() {
        md.push_str("## ℹ️ Informational Notices\n\n");
        for (idx, (rule_id, findings)) in notice_groups.iter().enumerate() {
            append_issue_block(&mut md, idx + 1, *rule_id, findings, include_urls);
        }
        md.push_str("---\n\n");
    }

    // 6. Actionable Quick Wins for Agent
    md.push_str("## 💡 Quick Wins for Agent\n");
    let mut win_idx = 1;
    for (rule_id, findings) in critical_groups.iter().chain(alert_groups.iter()).take(5) {
        let rule_def = get_rule(*rule_id);
        if !rule_def.fix_advice.is_empty() {
            md.push_str(&format!(
                "{}. {} (Affects {} page{})\n",
                win_idx,
                rule_def.fix_advice,
                findings.len(),
                if findings.len() == 1 { "" } else { "s" }
            ));
            win_idx += 1;
        }
    }

    md
}

fn append_issue_block(
    md: &mut String,
    num: usize,
    rule_id: RuleId,
    findings: &[&IssueFinding],
    include_urls: bool,
) {
    let rule_def = get_rule(rule_id);
    let count = findings.len();
    let count_suffix = if count == 1 { "page" } else { "pages" };

    md.push_str(&format!(
        "### {}. `{}` (Affects {} {})\n",
        num,
        rule_id.as_str(),
        count,
        count_suffix
    ));

    let problem_msg = findings
        .first()
        .map(|f| f.message.as_str())
        .unwrap_or(rule_def.title);
    md.push_str(&format!("- **Problem**: {}\n", problem_msg));

    if include_urls {
        md.push_str("- **Affected URLs**:\n");
        // Limit sample URLs to at most 3 to conserve agent context window
        for f in findings.iter().take(3) {
            md.push_str(&format!("  - `{}`\n", f.target_url));
        }
        if count > 3 {
            md.push_str(&format!(
                "  - *...and {} more pages (query with `seo_query_issues`)*\n",
                count - 3
            ));
        }
    }

    if !rule_def.fix_advice.is_empty() {
        md.push_str(&format!(
            "- **Action for Agent**: {}\n",
            rule_def.fix_advice
        ));
    }
    md.push('\n');
}
