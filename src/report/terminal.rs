//! # Terminal UI & Hacker-Style Executive Audit Matrix
//!
//! Provides a cyberpunk/matrix-inspired ANSI terminal user interface:
//! - Real-time in-place interactive loader with cybernetic progress bar.
//! - Cyberpunk ASCII art banner and startup telemetry box.
//! - Deep audit scorecard with visual health gauges, protocol radar, defect triage trees,
//!   PageRank authority distribution tables, and artifact links.

use crate::core::branding::{APP_DISPLAY_SPACED, APP_TAGLINE, BINARY_ALIAS, BINARY_NAME, VERSION};
use crate::core::models::{CrawlSummary, IssueFinding, Severity};
use crate::crawler::ai_check::{AiReadinessReport, AiSearchRisk};
use crate::crawler::engine::{CrawlResult, ProgressUpdate};
use crate::rules::page::schema_val::SchemaValidationOutcome;
use hashbrown::HashMap;
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

// Cyberpunk Pink & Maroon Palette (Black Sparrow)
const ANSI_PINK: &str = "\x1b[38;5;198m";
const ANSI_MAROON: &str = "\x1b[38;5;161m";
const ANSI_CYAN: &str = ANSI_PINK;
const ANSI_GREEN: &str = "\x1b[38;5;48m";
const ANSI_RED: &str = "\x1b[38;5;196m";
const ANSI_YELLOW: &str = "\x1b[38;5;220m";
const ANSI_DIM: &str = "\x1b[38;5;244m";
const ANSI_BRIGHT_WHITE: &str = "\x1b[38;5;231m";
const ANSI_BOLD: &str = "\x1b[1m";
const ANSI_RESET: &str = "\x1b[0m";

const BANNER_SPARROW: &str = "\
  ███████╗██████╗  █████╗ ██████╗ ██████╗  ██████╗ ██╗    ██╗\n\
  ██╔════╝██╔══██╗██╔══██╗██╔══██╗██╔══██╗██╔═══██╗██║    ██║\n\
  ███████╗██████╔╝███████║██████╔╝██████╔╝██║   ██║██║ █╗ ██║\n\
  ╚════██║██╔═══╝ ██╔══██║██╔══██╗██╔══██╗██║   ██║██║███╗██║\n\
  ███████║██║     ██║  ██║██║  ██║██║  ██║╚██████╔╝╚███╔███╔╝\n\
  ╚══════╝╚═╝     ╚═╝  ╚═╝╚═╝  ╚═╝╚═╝  ╚═╝ ╚═════╝  ╚══╝╚══╝";

fn format_section_header(title: &str) -> String {
    let pad = title.chars().count() + 4;
    let top = format!(
        "  {ANSI_BOLD}{ANSI_MAROON}┌{}┐{ANSI_RESET}\n",
        "─".repeat(pad)
    );
    let mid = format!(
        "  {ANSI_BOLD}{ANSI_MAROON}│{ANSI_RESET}  {ANSI_BOLD}{ANSI_BRIGHT_WHITE}{title}{ANSI_RESET}  {ANSI_BOLD}{ANSI_MAROON}│{ANSI_RESET}\n"
    );
    let bot = format!(
        "  {ANSI_BOLD}{ANSI_MAROON}└{}┘{ANSI_RESET}\n",
        "─".repeat(pad)
    );
    format!("{top}{mid}{bot}")
}

/// Splits `text` into lines where each line does not exceed `max_width`.
fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current_line = String::new();

    for word in text.split_whitespace() {
        if current_line.is_empty() {
            current_line.push_str(word);
        } else if current_line.len() + 1 + word.len() <= max_width {
            current_line.push(' ');
            current_line.push_str(word);
        } else {
            lines.push(current_line);
            current_line = word.to_string();
        }
    }

    if !current_line.is_empty() {
        lines.push(current_line);
    }

    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}

fn status_text(code: u16) -> &'static str {
    match code {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        410 => "Gone",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Unknown",
    }
}

static BANNER_PRINTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Prints the cyberpunk startup ASCII banner and mission parameters.
pub fn print_audit_banner(target_url: &str, max_pages: u32, concurrency: usize, aimd: bool) {
    BANNER_PRINTED.store(true, Ordering::SeqCst);
    let aimd_status = if aimd {
        format!("{ANSI_GREEN}ACTIVE (AIMD){ANSI_RESET}")
    } else {
        format!("{ANSI_YELLOW}STATIC{ANSI_RESET}")
    };
    let pages_limit = if max_pages > 0 {
        format!("{max_pages} pages")
    } else {
        "unlimited".to_string()
    };

    println!(
        "\n{ANSI_PINK}{ANSI_BOLD}{BANNER_SPARROW}{ANSI_RESET}\n  {ANSI_MAROON}{ANSI_BOLD}{APP_DISPLAY_SPACED}{ANSI_RESET}\n"
    );
    print!(
        "{}",
        format_section_header("BLACK SPARROW // DEEP AUDIT MATRIX")
    );
    println!("  {ANSI_BOLD}Target URL  {ANSI_RESET} : {ANSI_CYAN}{target_url}{ANSI_RESET}\n");
    println!("  {ANSI_BOLD}Parameters  {ANSI_RESET} : {pages_limit} {ANSI_DIM}│{ANSI_RESET} {concurrency} workers {ANSI_DIM}│{ANSI_RESET} AIMD: {aimd_status}\n");
}

/// Interactive single-line progress indicator for live crawl monitoring.
#[derive(Clone)]
pub struct CrawlProgressBar {
    max_pages: u32,
    start_time: Instant,
    tick_count: Arc<AtomicUsize>,
    last_draw_ms: Arc<AtomicU64>,
}

/// Creates a styled, single-line progress indicator for live crawl monitoring.
pub fn create_crawl_progress_bar(max_pages: u32) -> CrawlProgressBar {
    CrawlProgressBar {
        max_pages,
        start_time: Instant::now(),
        tick_count: Arc::new(AtomicUsize::new(0)),
        last_draw_ms: Arc::new(AtomicU64::new(0)),
    }
}

/// Updates the active progress bar with incoming telemetry strictly in-place on one line.
pub fn update_crawl_progress(pb: &CrawlProgressBar, update: &ProgressUpdate) {
    let now_ms = pb.start_time.elapsed().as_millis() as u64;
    let last = pb.last_draw_ms.load(Ordering::Relaxed);

    // Throttle redraws to at most once every 50ms, unless final page
    if pb.max_pages > 0
        && update.crawled_pages < pb.max_pages as usize
        && now_ms.saturating_sub(last) < 50
    {
        return;
    }
    pb.last_draw_ms.store(now_ms, Ordering::Relaxed);

    let elapsed_secs = pb.start_time.elapsed().as_secs();
    let mins = elapsed_secs / 60;
    let secs = elapsed_secs % 60;
    let elapsed_str = format!("{mins:02}:{secs:02}");

    let elapsed_f64 = pb.start_time.elapsed().as_secs_f64();
    let speed = if elapsed_f64 > 0.05 {
        (update.crawled_pages as f64 / elapsed_f64).round() as u64
    } else {
        0
    };

    const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let tick = pb.tick_count.fetch_add(1, Ordering::Relaxed);
    let frame = SPINNER[tick % SPINNER.len()];

    let total_target = if pb.max_pages > 0 {
        if update.discovered_pages > 0 {
            update.discovered_pages.min(pb.max_pages as usize)
        } else {
            pb.max_pages as usize
        }
    } else {
        update.discovered_pages
    };

    let bar_and_pct = if total_target > 0 {
        let pct = ((update.crawled_pages as f64 / total_target as f64) * 100.0).min(100.0) as usize;
        let filled = (pct * 14) / 100;
        let empty = 14 - filled;
        let filled_str = "▰".repeat(filled);
        let empty_str = "▱".repeat(empty);
        format!(
            "{ANSI_GREEN}[{filled_str}{ANSI_DIM}{empty_str}{ANSI_GREEN}]{ANSI_RESET} {ANSI_BOLD}{}/{}{ANSI_RESET} ({pct}%)",
            update.crawled_pages,
            total_target,
        )
    } else {
        format!("{ANSI_BOLD}{} pages{ANSI_RESET}", update.crawled_pages)
    };

    // Human-friendly defect indicators
    let issues_summary = if update.alert_count > 0 {
        format!(
            "{ANSI_RED}🚨 {}{ANSI_RESET} {ANSI_DIM}│{ANSI_RESET} {ANSI_YELLOW}⚠️ {}{ANSI_RESET} {ANSI_DIM}│{ANSI_RESET} {ANSI_YELLOW}⚡ {}{ANSI_RESET}",
            update.critical_count, update.alert_count, update.warning_count
        )
    } else {
        format!(
            "{ANSI_RED}🚨 {}{ANSI_RESET} {ANSI_DIM}│{ANSI_RESET} {ANSI_YELLOW}⚡ {}{ANSI_RESET}",
            update.critical_count, update.warning_count
        )
    };

    let line = format!(
        " {ANSI_CYAN}{ANSI_BOLD}{frame}{ANSI_RESET} {ANSI_DIM}[{elapsed_str}]{ANSI_RESET} {bar_and_pct} {ANSI_DIM}│{ANSI_RESET} {ANSI_CYAN}{speed} pages/s{ANSI_RESET} {ANSI_DIM}│{ANSI_RESET} {issues_summary}"
    );

    let mut out = io::stdout().lock();
    let _ = write!(out, "\r\x1b[2K{}", line);
    let _ = out.flush();
}

/// Finishes and clears the single-line progress indicator when crawling terminates.
pub fn finish_crawl_progress(_pb: &CrawlProgressBar) {
    let mut out = io::stdout().lock();
    let _ = write!(out, "\r\x1b[2K");
    let _ = out.flush();
}

/// Renders the comprehensive post-crawl executive scorecard in the terminal.
pub fn print_executive_scorecard(result: &CrawlResult, exported_paths: &[(&str, &Path)]) {
    let already_bannered = BANNER_PRINTED.swap(false, Ordering::SeqCst);
    if !already_bannered {
        print!(
            "\n{}",
            format_section_header("SEO LENS // DEEP AUDIT MATRIX")
        );
        println!(
            "  {ANSI_BOLD}Target URL  {ANSI_RESET} : {ANSI_CYAN}{}{ANSI_RESET}\n",
            result.target_url
        );
    }

    // Health Score Visual Gauge
    let filled_bars = (result.health_score as usize * 20) / 100;
    let empty_bars = 20 - filled_bars;
    let (score_color, rating) = if result.health_score >= 90 {
        (ANSI_GREEN, "EXCELLENT")
    } else if result.health_score >= 75 {
        (ANSI_YELLOW, "GOOD // MINOR DEFECTS")
    } else if result.health_score >= 50 {
        (ANSI_CYAN, "NEEDS ATTENTION")
    } else {
        (ANSI_RED, "CRITICAL DEFECTS DETECTED")
    };
    let filled_str = "█".repeat(filled_bars);
    let empty_str = "░".repeat(empty_bars);
    let gauge =
        format!("{score_color}[{filled_str}{ANSI_DIM}{empty_str}{score_color}]{ANSI_RESET}");
    println!(
        "  {ANSI_BOLD}Health Score{ANSI_RESET} : {gauge} {score_color}{ANSI_BOLD}{}/100 [{rating}]{ANSI_RESET}\n",
        result.health_score
    );

    // Latency Telemetry
    let avg_ttfb = if !result.pages.is_empty() {
        let mut ttfb_list: Vec<u32> = result.pages.iter().map(|p| p.ttfb_ms).collect();
        ttfb_list.sort_unstable();
        let avg = ttfb_list.iter().sum::<u32>() / ttfb_list.len() as u32;
        let p95_idx = ((ttfb_list.len() as f64 * 0.95).round() as usize).min(ttfb_list.len() - 1);
        let p95 = ttfb_list[p95_idx];
        let latency_desc = if avg < 100 {
            "Fast"
        } else if avg < 500 {
            "Moderate"
        } else {
            "Slow"
        };
        format!("{avg}ms (p95: {p95}ms) — {latency_desc}")
    } else {
        "0ms".to_string()
    };

    println!(
        "  {ANSI_BOLD}Telemetry   {ANSI_RESET} : {ANSI_GREEN}{:.1}s{ANSI_RESET} elapsed {ANSI_DIM}│{ANSI_RESET} {ANSI_GREEN}{}{ANSI_RESET} pages probed {ANSI_DIM}│{ANSI_RESET} {ANSI_GREEN}{}{ANSI_RESET} links indexed {ANSI_DIM}│{ANSI_RESET} TTFB: {ANSI_GREEN}{avg_ttfb}{ANSI_RESET}\n",
        result.duration.as_secs_f64(),
        result.pages.len(),
        result.graph.edge_count()
    );

    // HTTP Status Radar
    let mut status_counts: HashMap<u16, usize> = HashMap::new();
    for page in &result.pages {
        *status_counts.entry(page.status_code).or_default() += 1;
    }

    print!("{}", format_section_header("PROTOCOL TELEMETRY"));
    let mut sorted_statuses: Vec<_> = status_counts.into_iter().collect();
    sorted_statuses.sort_by_key(|k| k.0);

    for (status, count) in sorted_statuses {
        let pct = if !result.pages.is_empty() {
            (count as f64 / result.pages.len() as f64) * 100.0
        } else {
            0.0
        };
        let (icon, color) = match status {
            200..=299 => ("✔", ANSI_GREEN),
            300..=399 => ("ℹ", ANSI_CYAN),
            400..=499 => ("✖", ANSI_RED),
            _ => ("✖", ANSI_RED),
        };
        let reason = status_text(status);
        let status_desc = format!("{status} {reason}");
        println!(
            "  {color}{icon} {:<18}{ANSI_RESET} : {:>5} pages {ANSI_DIM}({:.1}%){ANSI_RESET}",
            status_desc, count, pct
        );
    }
    println!();

    // Top Priority Issues (Tree Format)
    print!("{}", format_section_header("DEFECT TRIAGE MATRIX"));
    if result.issues.is_empty() {
        println!("  {ANSI_GREEN}✔ Zero technical SEO defects detected across all probed nodes.{ANSI_RESET}\n");
    } else {
        let mut grouped: HashMap<
            crate::core::models::RuleId,
            Vec<&crate::core::models::IssueFinding>,
        > = HashMap::new();
        for issue in &result.issues {
            grouped.entry(issue.code).or_default().push(issue);
        }

        let mut sorted_groups: Vec<_> = grouped.into_iter().collect();
        sorted_groups.sort_by_key(|(_, list)| {
            match list.first().map(|i| i.severity).unwrap_or(Severity::Notice) {
                Severity::Critical => 0,
                Severity::Alert => 1,
                Severity::Warning => 2,
                Severity::Notice => 3,
            }
        });

        for (rule_id, findings) in sorted_groups.iter().take(6) {
            let sev = findings
                .first()
                .map(|i| i.severity)
                .unwrap_or(Severity::Notice);
            let (badge, color) = match sev {
                Severity::Critical => ("🚨 CRITICAL", ANSI_RED),
                Severity::Alert => ("⚠️ ALERT   ", ANSI_YELLOW),
                Severity::Warning => ("⚡ WARNING ", ANSI_YELLOW),
                Severity::Notice => ("ℹ️ NOTICE  ", ANSI_CYAN),
            };
            let rule_def = crate::rules::catalog::get_rule(*rule_id);
            let title = findings
                .first()
                .map(|i| i.title.as_str())
                .unwrap_or(rule_def.title);
            let remediation = rule_def.fix_advice;

            println!(
                "  {color}{ANSI_BOLD}▲ [{badge}] {}{ANSI_RESET} {ANSI_DIM}({} page{}){ANSI_RESET}",
                rule_id.as_str(),
                findings.len(),
                if findings.len() == 1 { "" } else { "s" }
            );

            let has_sample = !findings.is_empty();
            let has_remedy = !remediation.is_empty();

            // Defect line (wrapped at 58 chars to keep total line <= 76 chars)
            let defect_wrapped = wrap_text(title, 58);
            let defect_branch = if has_remedy || has_sample {
                "├──"
            } else {
                "└──"
            };
            let defect_cont = if has_remedy || has_sample { "│" } else { " " };
            println!(
                "    {ANSI_DIM}{defect_branch} Defect  :{ANSI_RESET} {}",
                defect_wrapped[0]
            );
            for cont in &defect_wrapped[1..] {
                println!("    {ANSI_DIM}{defect_cont}             {ANSI_RESET}{cont}");
            }

            // Remedy line (wrapped at 58 chars)
            if has_remedy {
                let remedy_wrapped = wrap_text(remediation, 58);
                let remedy_branch = if has_sample { "├──" } else { "└──" };
                let remedy_cont = if has_sample { "│" } else { " " };
                println!(
                    "    {ANSI_DIM}{remedy_branch} Remedy  :{ANSI_RESET} {}",
                    remedy_wrapped[0]
                );
                for cont in &remedy_wrapped[1..] {
                    println!("    {ANSI_DIM}{remedy_cont}             {ANSI_RESET}{cont}");
                }
            }

            // Sample line
            if let Some(sample) = findings.first() {
                println!(
                    "    {ANSI_DIM}└── Sample  :{ANSI_RESET} {ANSI_DIM}{}{ANSI_RESET}",
                    sample.target_url
                );
            }
            println!();
        }
    }

    // Top Authority Hubs // PageRank Distribution Matrix
    print!(
        "{}",
        format_section_header("TOP AUTHORITY HUBS // PAGERANK DISTRIBUTION")
    );
    println!("  {ANSI_DIM}Rank   Equity    Inlinks / Outlinks    Authority Node{ANSI_RESET}");
    println!("  {ANSI_DIM}─────  ──────    ──────────────────    ──────────────{ANSI_RESET}");

    let mut ranked_pages: Vec<_> = result
        .pages
        .iter()
        .map(|p| {
            let pr = result.pagerank.get(&p.url_hash).copied().unwrap_or(0.0);
            (p, pr)
        })
        .collect();
    ranked_pages.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    for (rank, (page, pr)) in ranked_pages.iter().take(5).enumerate() {
        let pct = pr * 100.0;
        println!(
            "   {ANSI_CYAN}{:02}.{ANSI_RESET}   {ANSI_GREEN}{:>5.2}%{ANSI_RESET}     {:>5} in / {:<3} out   {ANSI_CYAN}»{ANSI_RESET} {}",
            rank + 1,
            pct,
            result.graph.in_degree(&page.url),
            result.graph.out_degree(&page.url),
            page.url
        );
    }
    println!();

    // Exported Mission Artifacts
    if !exported_paths.is_empty() {
        print!("{}", format_section_header("GENERATED MISSION ARTIFACTS"));
        for (fmt, path) in exported_paths {
            println!("  {ANSI_CYAN}◈ {:<9}{ANSI_RESET} : {}", fmt, path.display());
        }
        println!();
    }
}

/// Renders the cyberpunk-styled historical crawl sessions in developer inspector aesthetic.
pub fn print_historical_sessions(db_path: &Path, crawls: &[CrawlSummary]) {
    // 1. Big Cyberpunk ASCII Header (matching inspect command)
    println!(
        "\n{ANSI_PINK}{ANSI_BOLD}{BANNER_SPARROW}{ANSI_RESET}\n  {ANSI_MAROON}{ANSI_BOLD}{APP_DISPLAY_SPACED}{ANSI_RESET}\n"
    );

    // 2. Persistence Repository
    print!("{}", format_section_header("PERSISTENCE REPOSITORY"));
    println!(
        "  {ANSI_BOLD}Database Path{ANSI_RESET} : {ANSI_CYAN}{}{ANSI_RESET}",
        db_path.display()
    );
    let session_count_str = match crawls.len() {
        0 => format!("{ANSI_YELLOW}0 sessions recorded{ANSI_RESET}"),
        1 => format!("{ANSI_GREEN}1 session recorded{ANSI_RESET}"),
        n => format!("{ANSI_GREEN}{n} sessions recorded{ANSI_RESET}"),
    };
    println!(
        "  {ANSI_BOLD}Total Audits {ANSI_RESET} : {session_count_str} {ANSI_DIM}│{ANSI_RESET} SQLite WAL Mode Active\n"
    );

    if crawls.is_empty() {
        println!("  {ANSI_YELLOW}⚡ No crawl sessions found in persistence.{ANSI_RESET}");
        println!("  Run {ANSI_CYAN}{ANSI_BOLD}{BINARY_NAME} audit <URL>{ANSI_RESET} to start your first technical SEO crawl.\n");
        return;
    }

    // 3. Historical Audit Sessions
    print!("{}", format_section_header("HISTORICAL AUDIT SESSIONS"));

    for (i, c) in crawls.iter().enumerate() {
        let status_badge = match c.status.to_lowercase().as_str() {
            "completed" => format!("{ANSI_GREEN}● COMPLETED{ANSI_RESET}"),
            "interrupted" => format!("{ANSI_YELLOW}▲ INTERRUPTED{ANSI_RESET}"),
            "crawling" => format!("{ANSI_CYAN}■ CRAWLING{ANSI_RESET}"),
            "failed" => format!("{ANSI_RED}✖ FAILED{ANSI_RESET}"),
            _ => {
                if c.finished_at.is_some() {
                    format!("{ANSI_GREEN}● COMPLETED{ANSI_RESET}")
                } else {
                    format!("{ANSI_YELLOW}▲ UNKNOWN{ANSI_RESET}")
                }
            }
        };

        let (score_color, rating) = if c.health_score >= 90 {
            (ANSI_GREEN, "Excellent")
        } else if c.health_score >= 75 {
            (ANSI_YELLOW, "Good // Minor Defects")
        } else if c.health_score >= 50 {
            (ANSI_CYAN, "Needs Attention")
        } else {
            (ANSI_RED, "Critical Defects Detected")
        };

        let started_clean = c.started_at.replace('T', " ");
        let started_short = if started_clean.len() >= 16 {
            &started_clean[..16]
        } else {
            &started_clean
        };

        let defects = if c.total_errors == 0 && c.total_alerts == 0 && c.total_warnings == 0 {
            format!("{ANSI_GREEN}✔ 0 defects{ANSI_RESET}")
        } else {
            format!(
                "{ANSI_RED}🚨 {} Critical{ANSI_RESET} {ANSI_DIM}│{ANSI_RESET} {ANSI_YELLOW}⚠️ {} Alerts{ANSI_RESET} {ANSI_DIM}│{ANSI_RESET} {ANSI_YELLOW}⚡ {} Warnings{ANSI_RESET}",
                c.total_errors, c.total_alerts, c.total_warnings
            )
        };

        println!(
            "  {ANSI_BOLD}{ANSI_CYAN}▸ {}{ANSI_RESET}  {status_badge}  {ANSI_DIM}[{started_short}]{ANSI_RESET}",
            c.session_id
        );
        println!(
            "    {ANSI_BOLD}Target URL  {ANSI_RESET} : {ANSI_CYAN}{}{ANSI_RESET}",
            c.target_url
        );
        println!(
            "    {ANSI_BOLD}Health Score{ANSI_RESET} : {score_color}{ANSI_BOLD}{}/100{ANSI_RESET} {ANSI_DIM}({rating}){ANSI_RESET}",
            c.health_score
        );
        println!(
            "    {ANSI_BOLD}Probed Nodes{ANSI_RESET} : {ANSI_BOLD}{}{ANSI_RESET} pages",
            c.total_pages_crawled
        );
        println!("    {ANSI_BOLD}Defect Triage{ANSI_RESET}: {defects}");

        if i < crawls.len() - 1 {
            println!();
        }
    }

    println!();
    // 4. Action Dispatch
    print!(
        "{}",
        format_section_header("ACTION DISPATCH // QUICK COMMANDS")
    );
    println!("  {ANSI_CYAN}◈ Re-inspect session {ANSI_RESET} : {BINARY_NAME} report <SESSION_ID>");
    println!("  {ANSI_CYAN}◈ Re-export artifacts{ANSI_RESET} : {BINARY_NAME} report <SESSION_ID> --format md,json");
    println!("  {ANSI_CYAN}◈ Start fresh crawl  {ANSI_RESET} : {BINARY_NAME} audit <URL>\n");
}

/// Formats and prints a filtered issues matrix in the terminal.
pub fn print_issues_matrix(
    session_id: &str,
    issues: &[IssueFinding],
    total_count: usize,
    offset: usize,
    limit: usize,
) {
    println!(
        "\n{ANSI_PINK}{ANSI_BOLD}{BANNER_SPARROW}{ANSI_RESET}\n  {ANSI_MAROON}{ANSI_BOLD}{APP_DISPLAY_SPACED}{ANSI_RESET}\n"
    );

    print!(
        "{}",
        format_section_header("AUDIT DEFECTS // FILTERED QUERY")
    );
    println!("  {ANSI_BOLD}Session ID{ANSI_RESET} : {ANSI_CYAN}{session_id}{ANSI_RESET}");
    println!(
        "  {ANSI_BOLD}Matching  {ANSI_RESET} : {total_count} issues found (showing {offset}..{})\n",
        (offset + issues.len()).min(total_count)
    );

    if issues.is_empty() {
        println!("  {ANSI_GREEN}✔ No issues matched your filter criteria.{ANSI_RESET}\n");
        return;
    }

    for (i, issue) in issues.iter().enumerate() {
        let (badge, color) = match issue.severity {
            Severity::Critical => (
                format!("{ANSI_RED}{ANSI_BOLD}[🚨 CRITICAL]{ANSI_RESET}"),
                ANSI_RED,
            ),
            Severity::Alert => (
                format!("{ANSI_YELLOW}{ANSI_BOLD}[⚠️ ALERT]{ANSI_RESET}"),
                ANSI_YELLOW,
            ),
            Severity::Warning => (
                format!("{ANSI_YELLOW}[⚡ WARNING]{ANSI_RESET}"),
                ANSI_YELLOW,
            ),
            Severity::Notice => (format!("{ANSI_CYAN}[ℹ NOTICE]{ANSI_RESET}"), ANSI_CYAN),
        };

        println!(
            "  {ANSI_BOLD}#{:03}{ANSI_RESET} {badge} {color}{ANSI_BOLD}{}{ANSI_RESET}",
            offset + i + 1,
            issue.code.as_str()
        );
        println!(
            "       {ANSI_BOLD}Target URL {ANSI_RESET}: {ANSI_CYAN}{}{ANSI_RESET}",
            issue.target_url
        );
        let diag_wrapped = wrap_text(&issue.message, 58);
        println!(
            "       {ANSI_BOLD}Diagnosis  {ANSI_RESET}: {}",
            diag_wrapped[0]
        );
        for cont in &diag_wrapped[1..] {
            println!("                  {cont}");
        }
        if let Some(ref src) = issue.source_page_url {
            println!("       {ANSI_BOLD}Source Page{ANSI_RESET}: {ANSI_DIM}{src}{ANSI_RESET}");
        }
        println!();
    }

    if total_count > offset + issues.len() {
        let next_offset = offset + limit;
        println!(
            "  {ANSI_DIM}◈ To view more: {BINARY_NAME} issues {session_id} --offset {next_offset} --limit {limit}{ANSI_RESET}\n"
        );
    }
}

/// Prints a cyberpunk AI search & GEO readiness assessment scorecard.
pub fn print_ai_readiness_scorecard(report: &AiReadinessReport) {
    println!(
        "\n{ANSI_PINK}{ANSI_BOLD}{BANNER_SPARROW}{ANSI_RESET}\n  {ANSI_MAROON}{ANSI_BOLD}{APP_DISPLAY_SPACED}{ANSI_RESET}\n"
    );

    let (risk_badge, risk_desc) = match report.citation_search_risk {
        AiSearchRisk::Low => (
            format!("{ANSI_GREEN}{ANSI_BOLD}[ LOW CITATION RISK ]{ANSI_RESET}"),
            format!("{ANSI_GREEN}Optimized for ChatGPT Search, Perplexity, and Claude{ANSI_RESET}"),
        ),
        AiSearchRisk::Medium => (
            format!("{ANSI_YELLOW}{ANSI_BOLD}[ MEDIUM CITATION RISK ]{ANSI_RESET}"),
            format!("{ANSI_YELLOW}Missing structured documentation (/llms.txt) or partial bot limits{ANSI_RESET}"),
        ),
        AiSearchRisk::High => (
            format!("{ANSI_RED}{ANSI_BOLD}[ HIGH CITATION RISK ]{ANSI_RESET}"),
            format!("{ANSI_RED}Critical AI search and retrieval engines blocked in robots.txt{ANSI_RESET}"),
        ),
    };

    print!(
        "{}",
        format_section_header("GENERATIVE ENGINE OPTIMIZATION (GEO)")
    );
    println!(
        "  {ANSI_BOLD}Target Domain{ANSI_RESET} : {ANSI_CYAN}{}{ANSI_RESET}",
        report.base_url
    );
    println!("  {ANSI_BOLD}Citation Risk{ANSI_RESET} : {risk_badge} - {risk_desc}\n");

    // 1. LLMS.TXT Artifacts
    print!("{}", format_section_header("LLMS.TXT PROTOCOL READINESS"));
    let llms_status = if report.llms_txt_found {
        format!("{ANSI_GREEN}✔ FOUND (200 OK){ANSI_RESET}")
    } else {
        format!("{ANSI_RED}✖ MISSING (404 NOT FOUND){ANSI_RESET}")
    };
    let llms_full_status = if report.llms_full_txt_found {
        format!("{ANSI_GREEN}✔ FOUND (200 OK){ANSI_RESET}")
    } else {
        format!("{ANSI_DIM}○ NOT PUBLISHED{ANSI_RESET}")
    };

    println!("  {ANSI_BOLD}/llms.txt     {ANSI_RESET} : {llms_status}");
    println!("  {ANSI_BOLD}/llms-full.txt{ANSI_RESET} : {llms_full_status}");
    if let Some(ref summary) = report.llms_txt_summary {
        println!("\n  {ANSI_DIM}Preview:{ANSI_RESET}");
        for line in summary.lines() {
            println!("    {ANSI_CYAN}{line}{ANSI_RESET}");
        }
    }
    println!();

    // 2. Real-Time Search & Retrieval Bots
    print!(
        "{}",
        format_section_header("REAL-TIME AI SEARCH & CITATION CRAWLERS")
    );
    for (bot, status) in &report.retrieval_bots {
        let status_str = if status == "ALLOWED" {
            format!("{ANSI_GREEN}ALLOWED ✔{ANSI_RESET}")
        } else {
            format!("{ANSI_RED}{ANSI_BOLD}DISALLOWED ✖{ANSI_RESET}")
        };
        println!("  {ANSI_BOLD}{:<18}{ANSI_RESET} : {status_str}", bot);
    }
    println!();

    // 3. AI Training Crawlers
    print!(
        "{}",
        format_section_header("AI FOUNDATION MODEL TRAINING BOTS")
    );
    for (bot, status) in &report.training_bots {
        let status_str = if status == "ALLOWED" {
            format!("{ANSI_GREEN}ALLOWED ✔{ANSI_RESET}")
        } else {
            format!("{ANSI_YELLOW}DISALLOWED ✖{ANSI_RESET}")
        };
        println!("  {ANSI_BOLD}{:<18}{ANSI_RESET} : {status_str}", bot);
    }
    println!();

    // 4. Actionable Recommendations
    if !report.recommendations.is_empty() {
        print!(
            "{}",
            format_section_header("GEO REMEDIATION // ACTION ITEMS")
        );
        for (idx, rec) in report.recommendations.iter().enumerate() {
            println!("  {ANSI_CYAN}◈ #{:02}{ANSI_RESET} : {rec}", idx + 1);
        }
        println!();
    }
}

/// Prints a schema validation assessment in the terminal.
pub fn print_schema_outcome(outcome: &SchemaValidationOutcome) {
    println!(
        "\n{ANSI_PINK}{ANSI_BOLD}{BANNER_SPARROW}{ANSI_RESET}\n  {ANSI_MAROON}{ANSI_BOLD}{APP_DISPLAY_SPACED}{ANSI_RESET}\n"
    );

    let (badge, color) = if outcome.is_rich_result_eligible {
        (
            format!("{ANSI_GREEN}{ANSI_BOLD}[ GOOGLE RICH RESULTS ELIGIBLE ✔ ]{ANSI_RESET}"),
            ANSI_GREEN,
        )
    } else {
        (
            format!("{ANSI_RED}{ANSI_BOLD}[ NOT RICH RESULTS ELIGIBLE ✖ ]{ANSI_RESET}"),
            ANSI_RED,
        )
    };

    print!(
        "{}",
        format_section_header("SCHEMA.ORG STRUCTURED DATA AUDIT")
    );
    println!(
        "  {ANSI_BOLD}Syntax Status{ANSI_RESET} : {}",
        if outcome.is_valid_json {
            format!("{ANSI_GREEN}Valid JSON-LD ✔{ANSI_RESET}")
        } else {
            format!("{ANSI_RED}Invalid JSON ✖{ANSI_RESET}")
        }
    );
    println!(
        "  {ANSI_BOLD}Detected @type{ANSI_RESET}: {ANSI_CYAN}{}{ANSI_RESET}",
        outcome.detected_type.as_deref().unwrap_or("Unknown")
    );
    println!("  {ANSI_BOLD}Eligibility  {ANSI_RESET} : {badge}\n");

    if !outcome.missing_required_fields.is_empty() {
        println!("  {ANSI_RED}{ANSI_BOLD}🚨 Missing Required Properties (Blocks Rich Results):{ANSI_RESET}");
        for field in &outcome.missing_required_fields {
            println!("    {ANSI_RED}✖ {field}{ANSI_RESET}");
        }
        println!();
    }

    if !outcome.missing_recommended_fields.is_empty() {
        println!("  {ANSI_YELLOW}⚡ Missing Recommended Properties (Enhances SERP Snippets):{ANSI_RESET}");
        for field in &outcome.missing_recommended_fields {
            println!("    {ANSI_YELLOW}○ {field}{ANSI_RESET}");
        }
        println!();
    }

    if outcome.is_rich_result_eligible && outcome.missing_recommended_fields.is_empty() {
        println!("  {color}✔ Schema passes all Google Rich Results and schema.org guidelines.{ANSI_RESET}\n");
    }
}

/// Prints the Cyberpunk / Matrix-style help and home screen for Black Sparrow CLI.
pub fn print_cli_help() {
    println!(
        "\n{ANSI_PINK}{ANSI_BOLD}{BANNER_SPARROW}{ANSI_RESET}\n  {ANSI_MAROON}{ANSI_BOLD}{APP_DISPLAY_SPACED}{ANSI_RESET} {ANSI_DIM}│{ANSI_RESET} {ANSI_BRIGHT_WHITE}v{VERSION} │ {APP_TAGLINE}{ANSI_RESET}\n"
    );

    let print_badge = |title: &str| {
        let pad = title.chars().count() + 4;
        println!(
            "  {ANSI_BOLD}{ANSI_MAROON}┌{}┐{ANSI_RESET}",
            "─".repeat(pad)
        );
        println!("  {ANSI_BOLD}{ANSI_MAROON}│{ANSI_RESET}  {ANSI_BOLD}{ANSI_BRIGHT_WHITE}{title}{ANSI_RESET}  {ANSI_BOLD}{ANSI_MAROON}│{ANSI_RESET}");
        println!(
            "  {ANSI_BOLD}{ANSI_MAROON}└{}┘{ANSI_RESET}",
            "─".repeat(pad)
        );
    };

    print_badge("USAGE");
    println!("  {ANSI_BOLD}{BINARY_NAME}{ANSI_RESET} {ANSI_CYAN}<COMMAND>{ANSI_RESET} {ANSI_YELLOW}[FLAGS]{ANSI_RESET} {ANSI_DIM}[OPTIONS]{ANSI_RESET}  {ANSI_DIM}(or alias: {BINARY_ALIAS} <COMMAND>){ANSI_RESET}\n");

    print_badge("AUDIT & CRAWL COMMANDS");
    println!("  {ANSI_GREEN}{ANSI_BOLD}audit{ANSI_RESET} {ANSI_CYAN}<URL>{ANSI_RESET}          Run full website crawl with AIMD adaptive congestion control");
    println!("  {ANSI_GREEN}{ANSI_BOLD}inspect{ANSI_RESET} {ANSI_CYAN}<URL>{ANSI_RESET}        Instant developer X-ray for a single webpage (DOM, tags, headers)");
    println!("  {ANSI_GREEN}{ANSI_BOLD}check-ai{ANSI_RESET} {ANSI_CYAN}<URL>{ANSI_RESET}       Audit AI search bot readiness (Perplexity, ChatGPT) & /llms.txt");
    println!("  {ANSI_GREEN}{ANSI_BOLD}schema{ANSI_RESET} {ANSI_CYAN}<TARGET>{ANSI_RESET}      Validate JSON-LD against Google Rich Results rules (file or URL)\n");

    print_badge("DATABASE & REPORTS");
    println!("  {ANSI_GREEN}{ANSI_BOLD}list{ANSI_RESET}                 List all historical crawl sessions stored in local SQLite DB");
    println!("  {ANSI_GREEN}{ANSI_BOLD}report{ANSI_RESET} {ANSI_CYAN}<ID>{ANSI_RESET}          Re-export or inspect an existing audit (terminal, md, json)");
    println!("  {ANSI_GREEN}{ANSI_BOLD}issues{ANSI_RESET} {ANSI_CYAN}<ID>{ANSI_RESET}          Drill down and filter audit findings by severity or category");
    println!("  {ANSI_GREEN}{ANSI_BOLD}delete{ANSI_RESET} {ANSI_CYAN}<ID>{ANSI_RESET}          Purge a specific crawl session and cascading records");
    println!("  {ANSI_GREEN}{ANSI_BOLD}clean{ANSI_RESET}                Clean historical crawl sessions older than N days\n");

    print_badge("AI AGENT PROTOCOL");
    println!("  {ANSI_GREEN}{ANSI_BOLD}mcp{ANSI_RESET}                  Start native Model Context Protocol server (stdio for AI agents)\n");

    print_badge("GLOBAL FLAGS");
    println!("  {ANSI_YELLOW}-h, --help{ANSI_RESET}           Print this help guide (or use: {BINARY_NAME} <cmd> --help)");
    println!("  {ANSI_YELLOW}-V, --version{ANSI_RESET}        Print version information\n");

    print_badge("QUICKSTART EXAMPLES");
    println!(
        "  {ANSI_DIM}# Audit whole website with 500 pages limit & AIMD rate limiting:{ANSI_RESET}"
    );
    println!("  {BINARY_NAME} audit https://example.com --max-pages 500\n");
    println!("  {ANSI_DIM}# Fast single-page inspection with JSON output:{ANSI_RESET}");
    println!("  {BINARY_ALIAS} inspect https://example.com/pricing --format json\n");
    println!(
        "  {ANSI_DIM}# Check whether AI bots (Perplexity, ChatGPT) can cite your site:{ANSI_RESET}"
    );
    println!("  {BINARY_ALIAS} check-ai https://example.com\n");
    println!("  {ANSI_DIM}# Filter critical issues from a previous crawl session:{ANSI_RESET}");
    println!("  {BINARY_NAME} issues <SESSION_ID> --severity critical\n");
    println!("  {ANSI_DIM}# Start MCP server for AI coding agents:{ANSI_RESET}");
    println!("  {BINARY_NAME} mcp\n");
}
