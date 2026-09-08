//! # Standalone Interactive Offline HTML Report Exporter
//!
//! Generates single-file self-contained HTML visual audit reports under `--format html`.
//!
//! Features:
//! - **100% Offline & Self-Contained**: All CSS and JavaScript are embedded directly in the HTML.
//!   Zero external CDN dependencies, web fonts, or tracking scripts.
//! - **Interactive UI**:
//!   - Live search filtering across pages and defect findings.
//!   - Severity filter chips (Critical, Alert, Warning, Notice).
//!   - Interactive tabbed navigation (Scorecard, Defect Triage, Pages Explorer, Authority Hubs).
//!   - Expandable issue accordions with "[Copy Prompt]" remediation assistance.
//!   - Crawl depth histogram and PageRank distribution.
//! - **Authentic Terminal Workstation / TUI Design**:
//!   - Monospace typography, ASCII branding banner, TUI panels, and clean CLI flag chips.

use crate::core::models::{PageReport, RobotsFlags, RuleId, Severity};
use crate::crawler::engine::CrawlResult;
use crate::error::{SeoError, SeoResult};
use crate::rules::catalog::get_rule;
use hashbrown::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Escapes HTML special characters to prevent document corruption or injection.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Evaluates page indexability for display in the pages explorer table.
fn evaluate_page_indexability(page: &PageReport) -> (&'static str, &'static str) {
    if !(200..=299).contains(&page.status_code) {
        return ("Non-Indexable", "badge-red");
    }
    if page.robots_flags.contains(RobotsFlags::NOINDEX) {
        return ("Noindex", "badge-yellow");
    }
    if let Some(canonical) = &page.canonical_url {
        let trimmed_c = canonical.trim();
        let trimmed_u = page.url.trim();
        if !trimmed_c.is_empty() && trimmed_c != trimmed_u {
            return ("Canonicalised", "badge-yellow");
        }
    }
    ("Indexable", "badge-green")
}

/// Exports the crawl result into a standalone interactive offline HTML report file.
pub fn export_html_report(result: &CrawlResult, output_dir: &Path) -> SeoResult<PathBuf> {
    fs::create_dir_all(output_dir)
        .map_err(|e| SeoError::Internal(format!("Failed to create output directory: {e}")))?;

    let host = url::Url::parse(&result.target_url)
        .map(|u| u.host_str().unwrap_or("site").replace('.', "_"))
        .unwrap_or_else(|_| "site_audit".to_string());

    let filename = format!("{host}_audit.html");
    let output_path = output_dir.join(filename);

    let html_content = render_html_report(result);

    fs::write(&output_path, html_content)
        .map_err(|e| SeoError::Internal(format!("Failed to write HTML report: {e}")))?;

    Ok(output_path)
}

/// Builds the complete HTML string containing styles, DOM, and interactive client scripts.
fn render_html_report(result: &CrawlResult) -> String {
    let target_esc = html_escape(&result.target_url);
    let session_id = result
        .pages
        .first()
        .map(|p| p.crawl_id.as_str())
        .unwrap_or("session_audit");

    // Dynamic ASCII track: 20 character block width
    let filled_blocks = ((result.health_score as usize * 20) / 100).min(20);
    let empty_blocks = 20 - filled_blocks;
    let ascii_bar = format!(
        "[{}{}] {}%",
        "█".repeat(filled_blocks),
        "░".repeat(empty_blocks),
        result.health_score
    );

    // Score rating & color
    let (score_color, score_badge, rating_text) = if result.health_score >= 90 {
        ("col-green", "badge-green", "Excellent // Production Ready")
    } else if result.health_score >= 75 {
        ("col-yellow", "badge-yellow", "Good // Minor Defects")
    } else if result.health_score >= 50 {
        ("col-blue", "badge-cyan", "Needs Attention")
    } else {
        ("col-red", "badge-red", "Critical Defects Detected")
    };

    // Severity counts
    let mut critical_count = 0;
    let mut alert_count = 0;
    let mut warning_count = 0;
    let mut notice_count = 0;
    for issue in &result.issues {
        match issue.severity {
            Severity::Critical => critical_count += 1,
            Severity::Alert => alert_count += 1,
            Severity::Warning => warning_count += 1,
            Severity::Notice => notice_count += 1,
        }
    }

    // Status code breakdown
    let mut status_counts: HashMap<u16, usize> = HashMap::new();
    for p in &result.pages {
        *status_counts.entry(p.status_code).or_default() += 1;
    }
    let mut sorted_statuses: Vec<_> = status_counts.into_iter().collect();
    sorted_statuses.sort_by_key(|k| k.0);

    // Crawl depth histogram
    let mut depth_counts: HashMap<u16, usize> = HashMap::new();
    for p in &result.pages {
        *depth_counts.entry(p.crawl_depth).or_default() += 1;
    }
    let mut sorted_depths: Vec<_> = depth_counts.into_iter().collect();
    sorted_depths.sort_by_key(|k| k.0);

    // Average TTFB
    let avg_ttfb = if !result.pages.is_empty() {
        let sum: u64 = result.pages.iter().map(|p| p.ttfb_ms as u64).sum();
        sum / (result.pages.len() as u64)
    } else {
        0
    };

    // Render Status Code Rows
    let mut status_rows = String::new();
    for (status, count) in &sorted_statuses {
        let pct = if !result.pages.is_empty() {
            (*count as f64 / result.pages.len() as f64) * 100.0
        } else {
            0.0
        };
        let badge_class = match status {
            200..=299 => "badge-green",
            300..=399 => "badge-cyan",
            400..=499 => "badge-red",
            _ => "badge-red",
        };
        status_rows.push_str(&format!(
            r#"<tr>
                <td><span class="badge {badge_class}">{status}</span></td>
                <td>{count} pages</td>
                <td>{pct:.1}%</td>
            </tr>"#
        ));
    }

    // Group issues by RuleId
    let mut grouped_issues: HashMap<RuleId, Vec<&crate::core::models::IssueFinding>> =
        HashMap::new();
    for issue in &result.issues {
        grouped_issues.entry(issue.code).or_default().push(issue);
    }
    let mut sorted_issue_groups: Vec<_> = grouped_issues.into_iter().collect();
    sorted_issue_groups.sort_by_key(|(_, list)| {
        match list.first().map(|i| i.severity).unwrap_or(Severity::Notice) {
            Severity::Critical => 0,
            Severity::Alert => 1,
            Severity::Warning => 2,
            Severity::Notice => 3,
        }
    });

    let mut issues_accordion_html = String::new();
    for (idx, (rule_id, findings)) in sorted_issue_groups.iter().enumerate() {
        let first = findings.first();
        let sev = first.map(|i| i.severity).unwrap_or(Severity::Notice);
        let sev_str = sev.as_str();
        let sev_display = match sev {
            Severity::Critical => "Critical",
            Severity::Alert => "Alert",
            Severity::Warning => "Warning",
            Severity::Notice => "Notice",
        };
        let rule_info = get_rule(*rule_id);
        let title_esc = html_escape(first.map(|i| i.title.as_str()).unwrap_or(rule_info.title));
        let category_esc = html_escape(rule_info.category.as_str());
        let advice_esc = html_escape(rule_info.fix_advice);
        let count = findings.len();

        let sev_badge = match sev {
            Severity::Critical => "badge-red",
            Severity::Alert => "badge-yellow",
            Severity::Warning => "badge-yellow",
            Severity::Notice => "badge-cyan",
        };

        let mut samples_html = String::new();
        for f in findings.iter().take(20) {
            let target = html_escape(&f.target_url);
            let msg = html_escape(&f.message);
            samples_html.push_str(&format!(
                r#"<li class="sample-item">
                    <span class="sample-url">{target}</span>
                    <span class="sample-msg">{msg}</span>
                </li>"#
            ));
        }
        if count > 20 {
            samples_html.push_str(&format!(
                r#"<li class="sample-more">... and {} more affected targets</li>"#,
                count - 20
            ));
        }

        issues_accordion_html.push_str(&format!(
            r#"<div class="issue-card" data-severity="{sev_str}" id="issue-{idx}">
                <div class="issue-header" onclick="toggleIssue({idx})">
                    <div class="issue-meta">
                        <span class="fold-caret">▶</span>
                        <span class="badge {sev_badge}">{sev_display}</span>
                        <span class="issue-code">{}</span>
                        <span class="badge badge-dim">{category_esc}</span>
                        <span class="issue-title">{title_esc}</span>
                    </div>
                    <div class="issue-count-pill">{count} target{}</div>
                </div>
                <div class="issue-body" id="issue-body-{idx}">
                    <div class="remediation-box">
                        <div class="remediation-header">
                            <span class="remediation-title">// Remediation Instruction</span>
                            <button class="copy-btn" onclick="copyRemedy(event, {idx})">[Copy Prompt]</button>
                        </div>
                        <div class="remediation-content" id="remedy-{idx}">{advice_esc}</div>
                    </div>
                    <div class="affected-pages-title">Affected Target URLs:</div>
                    <ul class="samples-list">{samples_html}</ul>
                </div>
            </div>"#,
            rule_id.as_str(),
            if count == 1 { "" } else { "s" }
        ));
    }

    // Render Pages Table Rows
    let mut pages_table_rows = String::new();
    for (i, p) in result.pages.iter().enumerate() {
        let url_esc = html_escape(&p.url);
        let title_esc = html_escape(p.title.as_deref().unwrap_or("-"));
        let h1_esc = html_escape(p.h1_primary.as_deref().unwrap_or("-"));
        let (indexability, ind_badge) = evaluate_page_indexability(p);
        let inlinks = result.graph.in_degree(&p.url);
        let outlinks = result.graph.out_degree(&p.url);

        let status_badge = match p.status_code {
            200..=299 => "badge-green",
            300..=399 => "badge-cyan",
            400..=499 => "badge-red",
            _ => "badge-red",
        };

        pages_table_rows.push_str(&format!(
            r#"<tr class="page-row" data-url="{url_esc}" data-title="{title_esc}" data-status="{}">
                <td class="col-num">{}</td>
                <td class="col-url"><a href="{url_esc}" target="_blank" rel="noopener noreferrer">{url_esc}</a></td>
                <td><span class="badge {status_badge}">{}</span></td>
                <td><span class="badge {ind_badge}">{indexability}</span></td>
                <td class="col-text">{title_esc}</td>
                <td class="col-text">{h1_esc}</td>
                <td>{inlinks}</td>
                <td>{outlinks}</td>
                <td>{}ms</td>
                <td>{}</td>
            </tr>"#,
            p.status_code,
            i + 1,
            p.status_code,
            p.ttfb_ms,
            p.word_count
        ));
    }

    // Render Authority Hubs (Top PageRank)
    let mut ranked_pages: Vec<_> = result
        .pages
        .iter()
        .map(|p| {
            let pr = result.pagerank.get(&p.url_hash).copied().unwrap_or(0.0);
            (p, pr)
        })
        .collect();
    ranked_pages.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let mut hubs_rows = String::new();
    for (rank, (p, pr)) in ranked_pages.iter().take(15).enumerate() {
        let url_esc = html_escape(&p.url);
        let inlinks = result.graph.in_degree(&p.url);
        let outlinks = result.graph.out_degree(&p.url);
        let pct = pr * 100.0;

        hubs_rows.push_str(&format!(
            r#"<tr>
                <td class="col-num">#{}</td>
                <td><span class="badge badge-green">{pct:.2}%</span></td>
                <td>{inlinks} in / {outlinks} out</td>
                <td class="col-url"><a href="{url_esc}" target="_blank" rel="noopener noreferrer">{url_esc}</a></td>
            </tr>"#,
            rank + 1
        ));
    }

    // Render Depth Histogram
    let max_depth_count = sorted_depths.iter().map(|(_, c)| *c).max().unwrap_or(1);
    let mut depth_bars_html = String::new();
    for (depth, count) in &sorted_depths {
        let width_pct = (*count as f64 / max_depth_count as f64) * 100.0;
        depth_bars_html.push_str(&format!(
            r#"<div class="depth-bar-row">
                <div class="depth-label">Depth {depth}:</div>
                <div class="depth-track">
                    <div class="depth-fill" style="width: {width_pct:.1}%;"></div>
                </div>
                <div class="depth-count">{count} pages</div>
            </div>"#
        ));
    }

    // Assemble complete self-contained HTML
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>SEO Lens Audit Report — {target_esc}</title>
    <style>
:root {{
            --bg-canvas: #090c10;
            --bg-term: #0d1117;
            --bg-panel: #161b22;
            --bg-header: #12171f;
            --bg-subtle: #21262d;
            --bg-hover: #1c2128;
            --bg-code: #0b0e14;
            --border-muted: #21262d;
            --border-panel: #30363d;
            --border-bright: #484f58;
            --text-main: #c9d1d9;
            --text-bright: #f0f6fc;
            --text-muted: #8b949e;
            --text-dim: #6e7681;
            --ansi-red: #f85149;
            --ansi-red-dim: #da3633;
            --ansi-red-bg: rgba(248, 81, 73, 0.12);
            --ansi-green: #3fb950;
            --ansi-green-dim: #238636;
            --ansi-green-bg: rgba(63, 185, 80, 0.12);
            --ansi-yellow: #d29922;
            --ansi-yellow-dim: #9e6a03;
            --ansi-yellow-bg: rgba(210, 153, 34, 0.12);
            --ansi-blue: #58a6ff;
            --ansi-blue-dim: #1f6feb;
            --ansi-blue-bg: rgba(88, 166, 255, 0.12);
            --ansi-cyan: #39c5cf;
            --ansi-cyan-bg: rgba(57, 197, 207, 0.12);
            --font-mono: ui-monospace, "SF Mono", "Cascadia Code", "JetBrains Mono", "Fira Code", Menlo, Monaco, Consolas, monospace;
        }}

        * {{
            box-sizing: border-box;
            margin: 0;
            padding: 0;
        }}

        body {{
            background-color: var(--bg-canvas);
            color: var(--text-main);
            font-family: var(--font-mono);
            font-size: 15px;
            line-height: 1.5;
            font-weight: 400;
            padding: 1.5rem 1rem;
            -webkit-font-smoothing: antialiased;
        }}

                /* Page Layout */
        .container {{
            max-width: 1440px;
            margin: 0 auto;
            padding: 1.75rem 1.5rem;
        }}

        /* TUI Branding Header */
        .tui-header {{
            display: flex;
            flex-direction: column;
            align-items: flex-start;
            gap: 1.25rem;
            border-bottom: 1px solid var(--border-panel);
            padding-bottom: 1.5rem;
            margin-bottom: 1.5rem;
        }}

        .ascii-logo {{
            font-family: var(--font-mono);
            font-size: 16px;
            line-height: 1.15;
            color: var(--ansi-blue);
            margin-bottom: 0.5rem;
            font-weight: 600;
        }}

        .brand-tagline {{
            font-size: 14px;
            color: var(--text-muted);
            font-family: var(--font-mono);
        }}

        .header-telemetry-card {{
            width: 100%;
            background: var(--bg-panel);
            border: 1px solid var(--border-panel);
            border-radius: 4px;
            padding: 0.75rem 1.25rem;
            display: flex;
            justify-content: space-between;
            align-items: center;
            flex-wrap: wrap;
            gap: 1rem;
            font-family: var(--font-mono);
        }}

        .target-row {{
            display: flex;
            align-items: center;
            gap: 0.65rem;
            font-size: 16px;
        }}

        .target-prompt {{
            color: var(--text-muted);
            font-size: 15px;
            font-weight: 500;
        }}

        .target-link {{
            color: var(--ansi-blue);
            text-decoration: none;
            font-weight: 500;
            font-size: 16.5px;
        }}

        .target-link:hover {{
            text-decoration: underline;
        }}

        .telemetry-badges {{
            display: flex;
            align-items: center;
            gap: 0.6rem;
            flex-wrap: wrap;
        }}

        .t-badge {{
            background: #0d1117;
            border: 1px solid var(--border-panel);
            border-radius: 4px;
            padding: 5px 11px;
            font-size: 14px;
            display: inline-flex;
            align-items: center;
            gap: 0.55rem;
        }}

        .tb-k {{
            color: var(--text-dim);
            font-size: 13px;
        }}

        .tb-v {{
            color: var(--text-bright);
            font-weight: 500;
            font-size: 14px;
        }}

        .tb-v.tb-alert {{
            color: var(--ansi-red);
        }}

        /* Panes & Telemetry */
        .telemetry-grid {{
            display: grid;
            grid-template-columns: 340px 1fr;
            gap: 1rem;
            margin-bottom: 1.25rem;
        }}

        @media (max-width: 960px) {{
            .telemetry-grid {{
                grid-template-columns: 1fr;
            }}
        }}

        .tui-pane {{
            background: var(--bg-panel);
            border: 1px solid var(--border-panel);
            border-radius: 4px;
            overflow: hidden;
            display: flex;
            flex-direction: column;
        }}

        .pane-header {{
            background: var(--bg-header);
            border-bottom: 1px solid var(--border-panel);
            padding: 0.5rem 0.95rem;
            display: flex;
            align-items: center;
            justify-content: space-between;
            font-size: 13px;
            color: var(--text-muted);
            font-weight: 500;
        }}

        .pane-title {{
            display: flex;
            align-items: center;
            gap: 0.4rem;
            color: var(--text-bright);
        }}

        .prompt-sym {{
            color: var(--ansi-blue);
            font-weight: 600;
        }}

        .pane-meta {{
            color: var(--text-dim);
            font-size: 12px;
        }}

        .pane-inner {{
            padding: 1rem 1.15rem;
            flex: 1;
        }}

        .score-box-term {{
            display: flex;
            flex-direction: column;
            gap: 0.45rem;
        }}

        .score-val-row {{
            display: flex;
            align-items: baseline;
            gap: 0.25rem;
        }}

        .score-num {{
            font-size: 4rem;
            font-weight: 600;
            line-height: 1;
            font-family: var(--font-mono);
        }}

        .score-denom {{
            font-size: 1.6rem;
            color: var(--text-dim);
            font-weight: 400;
        }}

        .score-ascii-track {{
            font-size: 15px;
            letter-spacing: 0.5px;
            color: var(--ansi-red);
            font-weight: 500;
            margin: 0.25rem 0;
        }}

        .score-verdict {{
            font-size: 13.5px;
            color: var(--text-muted);
            display: flex;
            align-items: center;
            gap: 0.4rem;
            margin-top: 0.25rem;
        }}

        .metrics-tui-grid {{
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
            gap: 0.75rem 1.25rem;
            align-content: center;
            height: 100%;
        }}

        .tui-stat-item {{
            display: flex;
            flex-direction: column;
            gap: 0.15rem;
        }}

        .stat-k {{
            font-size: 12.5px;
            color: var(--text-muted);
        }}

        .stat-v {{
            font-size: 16.5px;
            font-weight: 500;
            color: var(--text-bright);
        }}

        .col-red {{ color: var(--ansi-red) !important; }}
        .col-yellow {{ color: var(--ansi-yellow) !important; }}
        .col-green {{ color: var(--ansi-green) !important; }}
        .col-blue {{ color: var(--ansi-blue) !important; }}

        /* Terminal Multiplexer Navigation (Tmux / Zellij style) */
        .tmux-nav {{
            display: flex;
            gap: 0.25rem;
            background: #0b0f14;
            border: 1px solid var(--border-panel);
            border-radius: 4px;
            padding: 0.25rem;
            margin-bottom: 1.25rem;
            overflow-x: auto;
        }}

        .tmux-tab {{
            background: transparent;
            border: 1px solid transparent;
            color: var(--text-muted);
            font-family: var(--font-mono);
            font-size: 14px;
            font-weight: 500;
            padding: 0.4rem 0.85rem;
            cursor: pointer;
            border-radius: 3px;
            display: inline-flex;
            align-items: center;
            gap: 0.45rem;
            white-space: nowrap;
            transition: all 0.15s ease;
        }}

        .tmux-tab:hover {{
            color: var(--text-bright);
            background: var(--bg-subtle);
        }}

        .tmux-tab.active {{
            background: var(--bg-panel);
            color: var(--ansi-blue);
            border-color: var(--border-panel);
        }}

        .tmux-idx {{
            color: var(--text-dim);
        }}
        .tmux-tab.active .tmux-idx {{
            color: var(--ansi-green);
        }}

        .tmux-badge {{
            color: var(--text-dim);
            font-size: 12.5px;
        }}
        .tmux-tab.active .tmux-badge {{
            color: var(--text-muted);
        }}

        .tab-content {{
            display: none;
        }}
        .tab-content.active {{
            display: block;
        }}

        /* CLI Filter & Search Bar */
        .cli-filter-bar {{
            display: flex;
            align-items: center;
            justify-content: space-between;
            flex-wrap: wrap;
            gap: 0.75rem;
            background: var(--bg-panel);
            border: 1px solid var(--border-panel);
            border-radius: 4px;
            padding: 0.6rem 0.85rem;
            margin-bottom: 1rem;
        }}

        .cli-flags-group {{
            display: flex;
            align-items: center;
            gap: 0.4rem;
            flex-wrap: wrap;
        }}

        .cli-lead-lbl {{
            font-size: 13px;
            color: var(--text-dim);
            margin-right: 0.25rem;
        }}

        .cli-flag {{
            background: #0d1117;
            border: 1px solid var(--border-panel);
            color: var(--text-muted);
            font-family: var(--font-mono);
            font-size: 13px;
            font-weight: 500;
            padding: 0.25rem 0.6rem;
            border-radius: 3px;
            cursor: pointer;
            transition: all 0.15s ease;
        }}

        .cli-flag:hover {{
            color: var(--text-bright);
            border-color: var(--border-bright);
        }}

        .cli-flag.active {{
            background: var(--bg-subtle);
            color: var(--text-bright);
            border-color: var(--border-bright);
        }}

        .cli-flag.flag-crit.active {{
            border-color: var(--ansi-red-dim);
            color: var(--ansi-red);
            background: var(--ansi-red-bg);
        }}

        .cli-flag.flag-alert.active {{
            border-color: var(--ansi-yellow-dim);
            color: var(--ansi-yellow);
            background: var(--ansi-yellow-bg);
        }}

        .cli-flag.flag-warn.active {{
            border-color: var(--ansi-yellow-dim);
            color: var(--ansi-yellow);
            background: var(--ansi-yellow-bg);
        }}

        .cli-flag.flag-notice.active {{
            border-color: var(--ansi-blue-dim);
            color: var(--ansi-blue);
            background: var(--ansi-blue-bg);
        }}

        .grep-box {{
            display: flex;
            align-items: center;
            gap: 0.4rem;
            background: #0b0f14;
            border: 1px solid var(--border-panel);
            border-radius: 3px;
            padding: 0.25rem 0.6rem;
        }}

        .grep-prompt {{
            color: var(--ansi-green);
            font-size: 13px;
            font-weight: 500;
        }}

        .term-search-input {{
            background: transparent;
            border: none;
            color: var(--text-bright);
            font-family: var(--font-mono);
            font-size: 13.5px;
            width: 240px;
            outline: none;
        }}

        .term-search-input::placeholder {{
            color: var(--text-dim);
        }}

        /* Badges (Strictly Terminal Style) */
        .badge {{
            display: inline-block;
            font-family: var(--font-mono);
            font-size: 12.5px;
            font-weight: 500;
            padding: 1px 5px;
            border-radius: 2px;
            line-height: 1.35;
            
        }}

        .badge-green {{
            background: var(--ansi-green-bg);
            color: var(--ansi-green);
            border: 1px solid rgba(63, 185, 80, 0.35);
        }}

        .badge-yellow {{
            background: var(--ansi-yellow-bg);
            color: var(--ansi-yellow);
            border: 1px solid rgba(210, 153, 34, 0.35);
        }}

        .badge-cyan {{
            background: var(--ansi-blue-bg);
            color: var(--ansi-blue);
            border: 1px solid rgba(88, 166, 255, 0.35);
        }}

        .badge-red {{
            background: var(--ansi-red-bg);
            color: var(--ansi-red);
            border: 1px solid rgba(248, 81, 73, 0.35);
        }}

        .badge-dim {{
            background: #21262d;
            color: var(--text-muted);
            border: 1px solid var(--border-panel);
        }}

        /* Issue Cards (Terminal Log & Fold Hierarchy) */
        .issue-card {{
            background: var(--bg-panel);
            border: 1px solid var(--border-panel);
            border-radius: 4px;
            margin-bottom: 0.5rem;
            overflow: hidden;
        }}

        .issue-header {{
            padding: 0.65rem 0.85rem;
            display: flex;
            align-items: center;
            justify-content: space-between;
            cursor: pointer;
            user-select: none;
            background: var(--bg-panel);
            transition: background 0.12s ease;
            gap: 0.75rem;
        }}

        .issue-header:hover {{
            background: var(--bg-hover);
        }}

        .issue-meta {{
            display: flex;
            align-items: center;
            gap: 0.55rem;
            flex-wrap: wrap;
        }}

        .fold-caret {{
            color: var(--text-dim);
            font-size: 12px;
            width: 10px;
            display: inline-block;
            transition: transform 0.15s ease;
        }}

        .issue-card.open .fold-caret {{
            transform: rotate(90deg);
            color: var(--ansi-blue);
        }}

        .issue-code {{
            font-size: 13.5px;
            font-weight: 500;
            color: var(--ansi-blue);
        }}

        .issue-title {{
            font-size: 14px;
            font-weight: 400;
            color: var(--text-bright);
        }}

        .issue-count-pill {{
            font-size: 12.5px;
            color: var(--text-muted);
            background: #0d1117;
            border: 1px solid var(--border-panel);
            padding: 1px 6px;
            border-radius: 2px;
            white-space: nowrap;
        }}

        .issue-body {{
            display: none;
            padding: 1rem 1.25rem;
            border-top: 1px solid var(--border-panel);
            background: #0b0e14;
        }}

        /* Remediation Box */
        .remediation-box {{
            background: var(--bg-code);
            border: 1px solid var(--border-panel);
            border-left: 3px solid var(--ansi-blue);
            border-radius: 2px;
            padding: 0.75rem 1rem;
            margin-bottom: 1rem;
        }}

        .remediation-header {{
            display: flex;
            justify-content: space-between;
            align-items: center;
            margin-bottom: 0.4rem;
        }}

        .remediation-title {{
            font-size: 13px;
            font-weight: 500;
            color: var(--ansi-green);
        }}

        .remediation-content {{
            font-size: 14px;
            color: var(--text-bright);
            line-height: 1.5;
        }}

        .copy-btn {{
            background: #21262d;
            color: var(--text-muted);
            border: 1px solid var(--border-panel);
            padding: 0.25rem 0.65rem;
            border-radius: 2px;
            font-family: var(--font-mono);
            font-size: 12.5px;
            font-weight: 500;
            cursor: pointer;
            transition: all 0.15s ease;
        }}

        .copy-btn:hover {{
            color: var(--text-bright);
            border-color: var(--border-bright);
            background: #30363d;
        }}

        /* Affected URL Tree Samples */
        .affected-pages-title {{
            font-size: 13px;
            color: var(--text-muted);
            margin-bottom: 0.5rem;
            font-weight: 500;
        }}

        .samples-list {{
            list-style: none;
            display: flex;
            flex-direction: column;
            gap: 0.35rem;
        }}

        .sample-item {{
            display: flex;
            flex-direction: column;
            gap: 0.15rem;
            font-size: 13.5px;
            line-height: 1.4;
            padding-left: 0.25rem;
        }}

        .sample-url {{
            color: var(--ansi-blue);
            word-break: break-all;
        }}
        .sample-url::before {{
            content: "├── ";
            color: var(--text-dim);
        }}

        .sample-msg {{
            color: var(--text-muted);
            padding-left: 1.6rem;
            font-size: 13px;
        }}
        .sample-msg::before {{
            content: "└── ";
            color: var(--border-bright);
        }}

        .sample-more {{
            font-size: 13px;
            color: var(--text-dim);
            padding-left: 1.6rem;
            margin-top: 0.25rem;
        }}
        .sample-more::before {{
            content: "└── ... ";
            color: var(--text-dim);
        }}

        /* Tables (Curated Terminal Matrix) */
        .table-wrap {{
            overflow-x: auto;
            border: 1px solid var(--border-panel);
            border-radius: 3px;
            background: var(--bg-panel);
        }}

        table {{
            width: 100%;
            border-collapse: collapse;
            text-align: left;
            font-size: 13.5px;
        }}

        th {{
            background: #12171f;
            color: var(--text-muted);
            font-weight: 500;
            padding: 0.65rem 0.95rem;
            border-bottom: 1px solid var(--border-panel);
            font-size: 12.5px;
            white-space: nowrap;
        }}

        td {{
            padding: 0.6rem 0.95rem;
            border-bottom: 1px solid #1e2430;
            color: var(--text-main);
            white-space: nowrap;
        }}

        tr:last-child td {{
            border-bottom: none;
        }}

        tr:hover td {{
            background: var(--bg-hover);
        }}

        .col-num {{
            width: 36px;
            color: var(--text-dim);
        }}

        .col-url {{
            max-width: 380px;
            overflow: hidden;
            text-overflow: ellipsis;
            white-space: nowrap;
        }}

        .col-url a {{
            color: var(--ansi-blue);
            text-decoration: none;
        }}

        .col-url a:hover {{
            text-decoration: underline;
        }}

        .col-text {{
            max-width: 220px;
            overflow: hidden;
            text-overflow: ellipsis;
            white-space: nowrap;
            color: var(--text-muted);
        }}

        /* Depth Bar Graph in Architecture Tab */
        .depth-box {{
            display: flex;
            flex-direction: column;
            gap: 0.6rem;
        }}

        .depth-bar-row {{
            display: flex;
            align-items: center;
            gap: 0.75rem;
            font-size: 12px;
        }}

        .depth-label {{
            width: 75px;
            color: var(--text-muted);
        }}

        .depth-track {{
            flex: 1;
            background: #0d1117;
            height: 10px;
            border: 1px solid var(--border-panel);
            border-radius: 2px;
            overflow: hidden;
        }}

        .depth-fill {{
            background: var(--ansi-green);
            height: 100%;
        }}

        .depth-count {{
            width: 80px;
            text-align: right;
            color: var(--text-bright);
        }}
    </style>
</head>
<body>
    <div class="container">
        <!-- TUI Branding Header -->
        <header class="tui-header">
            <div class="brand-block">
                <pre class="ascii-logo">  ███████╗███████╗ ██████╗     ██╗     ███████╗███╗   ██╗███████╗
  ██╔════╝██╔════╝██╔═══██╗    ██║     ██╔════╝████╗  ██║██╔════╝
  ███████╗█████╗  ██║   ██║    ██║     █████╗  ██╔██╗ ██║███████╗
  ╚════██║██╔══╝  ██║   ██║    ██║     ██╔══╝  ██║╚██╗██║╚════██║
  ███████║███████╗╚██████╔╝    ███████╗███████╗██║ ╚████║███████║
  ╚══════╝╚══════╝ ╚═════╝     ╚══════╝╚══════╝╚═╝  ╚═══╝╚══════╝</pre>
                <div class="brand-tagline">v0.1.0-rc.1 │ High-Performance Website Crawler &amp; AI-Native Technical SEO Engine</div>
            </div>
            <div class="header-telemetry-card">
                <div class="target-row">
                    <span class="target-prompt"><span class="prompt-sym">$</span> Target:</span>
                    <a href="{target_esc}" target="_blank" rel="noopener noreferrer" class="target-link">{target_esc}</a>
                </div>
                <div class="telemetry-badges">
                    <span class="t-badge"><span class="tb-k">session</span><span class="tb-v">{session_id}</span></span>
                    <span class="t-badge"><span class="tb-k">probed</span><span class="tb-v">{} targets</span></span>
                    <span class="t-badge"><span class="tb-k">findings</span><span class="tb-v tb-alert">{}</span></span>
                    <span class="t-badge"><span class="tb-k">duration</span><span class="tb-v">{:.1}s</span></span>
                </div>
            </div>
        </header>

        <main>
            <!-- Executive Telemetry & Scorecard -->
            <section class="telemetry-grid">
                <div class="tui-pane">
                    <div class="pane-header">
                        <span class="pane-title"><span class="prompt-sym">//</span> Health Score</span>
                        <span class="pane-meta">Rating: {}/100</span>
                    </div>
                    <div class="pane-inner">
                        <div class="score-box-term">
                            <div class="score-val-row">
                                <span class="score-num {score_color}">{}</span>
                                <span class="score-denom">/100</span>
                            </div>
                            <div class="score-ascii-track">{ascii_bar}</div>
                            <div class="score-verdict">
                                <span>Verdict:</span> <span class="badge {score_badge}">{rating_text}</span>
                            </div>
                        </div>
                    </div>
                </div>

                <div class="tui-pane">
                    <div class="pane-header">
                        <span class="pane-title"><span class="prompt-sym">//</span> Crawl Telemetry</span>
                        <span class="pane-meta">Session: {session_id}</span>
                    </div>
                    <div class="pane-inner">
                        <div class="metrics-tui-grid">
                            <div class="tui-stat-item">
                                <span class="stat-k">Targets Probed:</span>
                                <span class="stat-v">{} pages</span>
                            </div>
                            <div class="tui-stat-item">
                                <span class="stat-k">Internal Links:</span>
                                <span class="stat-v">{} edges</span>
                            </div>
                            <div class="tui-stat-item">
                                <span class="stat-k">Average TTFB:</span>
                                <span class="stat-v">{avg_ttfb}ms</span>
                            </div>
                            <div class="tui-stat-item">
                                <span class="stat-k">Crawl Duration:</span>
                                <span class="stat-v">{:.1}s</span>
                            </div>
                            <div class="tui-stat-item">
                                <span class="stat-k">Critical Defects:</span>
                                <span class="stat-v col-red">{critical_count}</span>
                            </div>
                            <div class="tui-stat-item">
                                <span class="stat-k">Alerts &amp; Warnings:</span>
                                <span class="stat-v col-yellow">{}</span>
                            </div>
                        </div>
                    </div>
                </div>
            </section>

            <!-- Multiplexer Navigation Tabs (Tmux / Zellij style) -->
            <nav class="tmux-nav">
                <button class="tmux-tab active" onclick="switchTab('tab-issues', event)">
                    <span class="tmux-idx">1:</span> Defect Triage <span class="tmux-badge">[{}]</span>
                </button>
                <button class="tmux-tab" onclick="switchTab('tab-pages', event)">
                    <span class="tmux-idx">2:</span> All Pages <span class="tmux-badge">[{}]</span>
                </button>
                <button class="tmux-tab" onclick="switchTab('tab-overview', event)">
                    <span class="tmux-idx">3:</span> Status Codes <span class="tmux-badge">[HTTP]</span>
                </button>
                <button class="tmux-tab" onclick="switchTab('tab-graph', event)">
                    <span class="tmux-idx">4:</span> Site Architecture <span class="tmux-badge">[Rank]</span>
                </button>
            </nav>

            <!-- Tab 1: Defect Triage & Issues -->
            <section id="tab-issues" class="tab-content active">
                <div class="cli-filter-bar">
                    <div class="cli-flags-group">
                        <span class="cli-lead-lbl">Severity:</span>
                        <button class="cli-flag active" onclick="filterSeverity('all', event)">All [{}]</button>
                        <button class="cli-flag flag-crit" onclick="filterSeverity('critical', event)">Critical [{critical_count}]</button>
                        <button class="cli-flag flag-alert" onclick="filterSeverity('alert', event)">Alert [{alert_count}]</button>
                        <button class="cli-flag flag-warn" onclick="filterSeverity('warning', event)">Warning [{warning_count}]</button>
                        <button class="cli-flag flag-notice" onclick="filterSeverity('notice', event)">Notice [{notice_count}]</button>
                    </div>
                    <div class="grep-box">
                        <span class="grep-prompt">grep:</span>
                        <input type="text" id="issueSearch" class="term-search-input" placeholder="Filter rule or URL..." onkeyup="searchIssues()">
                    </div>
                </div>
                <div id="issuesContainer">
                    {issues_accordion_html}
                </div>
            </section>

            <!-- Tab 2: All Pages Explorer -->
            <section id="tab-pages" class="tab-content">
                <div class="cli-filter-bar">
                    <div class="grep-box">
                        <span class="grep-prompt">grep:</span>
                        <input type="text" id="pageSearch" class="term-search-input" style="width: 320px;" placeholder="Filter URL, title, status..." onkeyup="searchPages()">
                    </div>
                    <div style="font-size: 12.5px; color: var(--text-muted);" id="pageCountDisplay">Showing {} / {} targets</div>
                </div>
                <div class="table-wrap">
                    <table>
                        <thead>
                            <tr>
                                <th class="col-num">#</th>
                                <th>Page URL</th>
                                <th>Status</th>
                                <th>Indexability</th>
                                <th>Page Title</th>
                                <th>Primary H1</th>
                                <th>Inlinks</th>
                                <th>Outlinks</th>
                                <th>TTFB</th>
                                <th>Words</th>
                            </tr>
                        </thead>
                        <tbody id="pagesTableBody">
                            {pages_table_rows}
                        </tbody>
                    </table>
                </div>
            </section>

            <!-- Tab 3: Protocol Telemetry -->
            <section id="tab-overview" class="tab-content">
                <div class="tui-pane" style="max-width: 600px;">
                    <div class="pane-header">
                        <span class="pane-title"><span class="prompt-sym">//</span> HTTP Status Code Distribution</span>
                        <span class="pane-meta">{} codes</span>
                    </div>
                    <div class="pane-inner">
                        <div class="table-wrap">
                            <table>
                                <thead>
                                    <tr>
                                        <th>Status</th>
                                        <th>Targets</th>
                                        <th>Ratio</th>
                                    </tr>
                                </thead>
                                <tbody>
                                    {status_rows}
                                </tbody>
                            </table>
                        </div>
                    </div>
                </div>
            </section>

            <!-- Tab 4: Site Architecture & Hubs -->
            <section id="tab-graph" class="tab-content">
                <div style="display: grid; grid-template-columns: repeat(auto-fit, minmax(400px, 1fr)); gap: 1rem;">
                    <div class="tui-pane">
                        <div class="pane-header">
                            <span class="pane-title"><span class="prompt-sym">//</span> Top Authority Hubs (PageRank)</span>
                            <span class="pane-meta">Top 15 Nodes</span>
                        </div>
                        <div class="pane-inner">
                            <div class="table-wrap">
                                <table>
                                    <thead>
                                        <tr>
                                            <th class="col-num">Rank</th>
                                            <th>Equity</th>
                                            <th>In / Out</th>
                                            <th>Authority Target URL</th>
                                        </tr>
                                    </thead>
                                    <tbody>
                                        {hubs_rows}
                                    </tbody>
                                </table>
                            </div>
                        </div>
                    </div>
                    <div class="tui-pane">
                        <div class="pane-header">
                            <span class="pane-title"><span class="prompt-sym">//</span> Crawl Depth Hierarchy</span>
                            <span class="pane-meta">Max Depth: {}</span>
                        </div>
                        <div class="pane-inner">
                            <div class="depth-box">
                                {depth_bars_html}
                            </div>
                        </div>
                    </div>
                </div>
            </section>
        </main>
    </div>

    <script>
        function switchTab(tabId, evt) {{
            document.querySelectorAll('.tab-content').forEach(el => el.classList.remove('active'));
            document.querySelectorAll('.tmux-tab').forEach(el => el.classList.remove('active'));
            const target = document.getElementById(tabId);
            if (target) target.classList.add('active');
            const e = evt || (typeof event !== 'undefined' ? event : null);
            if (e && e.currentTarget) {{
                e.currentTarget.classList.add('active');
            }}
        }}

        function toggleIssue(idx) {{
            const card = document.getElementById('issue-' + idx);
            const body = document.getElementById('issue-body-' + idx);
            if (card && body) {{
                const isOpen = card.classList.contains('open');
                if (isOpen) {{
                    card.classList.remove('open');
                    body.style.display = 'none';
                }} else {{
                    card.classList.add('open');
                    body.style.display = 'block';
                }}
            }}
        }}

        let currentSeverity = 'all';
        function filterSeverity(sev, evt) {{
            currentSeverity = sev;
            document.querySelectorAll('.cli-flag').forEach(c => c.classList.remove('active'));
            const e = evt || (typeof event !== 'undefined' ? event : null);
            if (e && e.currentTarget) {{
                e.currentTarget.classList.add('active');
            }}
            searchIssues();
        }}

        function searchIssues() {{
            const query = (document.getElementById('issueSearch').value || '').toLowerCase();
            const cards = document.querySelectorAll('.issue-card');
            cards.forEach(card => {{
                const sev = card.getAttribute('data-severity');
                const text = card.innerText.toLowerCase();
                const matchesSev = (currentSeverity === 'all' || sev === currentSeverity);
                const matchesQuery = query === '' || text.includes(query);
                card.style.display = (matchesSev && matchesQuery) ? 'block' : 'none';
            }});
        }}

        function searchPages() {{
            const query = (document.getElementById('pageSearch').value || '').toLowerCase();
            const rows = document.querySelectorAll('.page-row');
            let visible = 0;
            rows.forEach(row => {{
                const text = row.innerText.toLowerCase();
                const match = query === '' || text.includes(query);
                row.style.display = match ? '' : 'none';
                if (match) visible++;
            }});
            const countDisplay = document.getElementById('pageCountDisplay');
            if (countDisplay) {{
                countDisplay.innerText = 'showing ' + visible + ' / ' + rows.length + ' targets';
            }}
        }}

        function copyRemedy(event, idx) {{
            event.stopPropagation();
            const el = document.getElementById('remedy-' + idx);
            if (!el) return;
            const text = el.innerText;
            navigator.clipboard.writeText(text).then(() => {{
                const btn = event.currentTarget || event.target;
                const orig = btn.innerText;
                btn.innerText = '[copied!]';
                btn.style.color = 'var(--ansi-green)';
                btn.style.borderColor = 'var(--ansi-green)';
                setTimeout(() => {{
                    btn.innerText = orig;
                    btn.style.color = '';
                    btn.style.borderColor = '';
                }}, 2000);
            }}).catch(err => {{
                console.error('Copy failed: ', err);
            }});
        }}
    </script>
</body>
</html>"#,
        result.pages.len(),
        result.issues.len(),
        result.duration.as_secs_f64(),
        result.health_score,
        result.health_score,
        result.pages.len(),
        result.graph.edge_count(),
        result.duration.as_secs_f64(),
        alert_count + warning_count,
        result.issues.len(),
        result.pages.len(),
        result.issues.len(),
        result.pages.len(),
        result.pages.len(),
        sorted_statuses.len(),
        sorted_depths.last().map(|(d, _)| *d).unwrap_or(0)
    )
}
