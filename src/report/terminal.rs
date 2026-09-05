//! # Terminal UI & Hacker-Style Executive Audit Matrix
//!
//! Provides a cyberpunk/matrix-inspired ANSI terminal user interface:
//! - Real-time in-place interactive loader with cybernetic progress bar.
//! - Cyberpunk ASCII art banner and startup telemetry box.
//! - Deep audit scorecard with visual health gauges, protocol radar, defect triage trees,
//!   PageRank authority distribution tables, and artifact links.

use crate::core::models::Severity;
use crate::crawler::engine::{CrawlResult, ProgressUpdate};
use hashbrown::HashMap;
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Instant;

// Cyberpunk / Hacker 256-Color & ANSI escape codes
const ANSI_CYAN: &str = "\x1b[38;5;51m";
const ANSI_GREEN: &str = "\x1b[38;5;48m";
const ANSI_RED: &str = "\x1b[38;5;196m";
const ANSI_YELLOW: &str = "\x1b[38;5;220m";
const ANSI_DIM: &str = "\x1b[38;5;244m";
const ANSI_BOLD: &str = "\x1b[1m";
const ANSI_RESET: &str = "\x1b[0m";

/// Prints the cyberpunk startup ASCII banner and mission parameters.
pub fn print_audit_banner(target_url: &str, max_pages: u32, concurrency: usize, aimd: bool) {
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
        "\n{ANSI_CYAN}{ANSI_BOLD}  ███████╗███████╗ ██████╗     ██╗     ███████╗███╗   ██╗███████╗\n  ██╔════╝██╔════╝██╔═══██╗    ██║     ██╔════╝████╗  ██║██╔════╝\n  ███████╗█████╗  ██║   ██║    ██║     █████╗  ██╔██╗ ██║███████╗\n  ╚════██║██╔══╝  ██║   ██║    ██║     ██╔══╝  ██║╚██╗██║╚════██║\n  ███████║███████╗╚██████╔╝    ███████╗███████╗██║ ╚████║███████║\n  ╚══════╝╚══════╝ ╚═════╝     ╚══════╝╚══════╝╚═╝  ╚═══╝╚══════╝{ANSI_RESET}\n\n{ANSI_DIM}┌──[{ANSI_RESET} {ANSI_BOLD}TARGET TELEMETRY{ANSI_RESET} {ANSI_DIM}]────────────────────────────────────────────────────────{ANSI_RESET}\n{ANSI_DIM}│{ANSI_RESET}  {ANSI_BOLD}Target URL  {ANSI_RESET} : {ANSI_CYAN}{target_url}{ANSI_RESET}\n{ANSI_DIM}│{ANSI_RESET}  {ANSI_BOLD}Parameters  {ANSI_RESET} : {pages_limit} {ANSI_DIM}│{ANSI_RESET} {concurrency} workers {ANSI_DIM}│{ANSI_RESET} AIMD: {aimd_status}\n{ANSI_DIM}└────────────────────────────────────────────────────────────────────────┘{ANSI_RESET}\n"
    );
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
    println!(
        "\n{ANSI_CYAN}╔══════════════════════════════════════════════════════════════════════════╗"
    );
    println!("║                    SEO LENS // DEEP AUDIT MATRIX                         ║");
    println!(
        "╚══════════════════════════════════════════════════════════════════════════╝{ANSI_RESET}"
    );
    println!(
        "  {ANSI_BOLD}Target{ANSI_RESET}       : {ANSI_CYAN}{}{ANSI_RESET}",
        result.target_url
    );

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
        "  {ANSI_BOLD}Health Score{ANSI_RESET} : {gauge} {score_color}{ANSI_BOLD}{}/100 [{rating}]{ANSI_RESET}",
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
        "  {ANSI_BOLD}Telemetry{ANSI_RESET}    : {ANSI_GREEN}{:.1}s{ANSI_RESET} elapsed {ANSI_DIM}│{ANSI_RESET} {ANSI_GREEN}{}{ANSI_RESET} pages probed {ANSI_DIM}│{ANSI_RESET} {ANSI_GREEN}{}{ANSI_RESET} links indexed {ANSI_DIM}│{ANSI_RESET} TTFB: {ANSI_GREEN}{avg_ttfb}{ANSI_RESET}\n",
        result.duration.as_secs_f64(),
        result.pages.len(),
        result.graph.edge_count()
    );

    // HTTP Status Radar
    let mut status_counts: HashMap<u16, usize> = HashMap::new();
    for page in &result.pages {
        *status_counts.entry(page.status_code).or_default() += 1;
    }

    println!("{ANSI_DIM}┌──[{ANSI_RESET} {ANSI_BOLD}PROTOCOL TELEMETRY{ANSI_RESET} {ANSI_DIM}]───────────────────────────────────────────────────{ANSI_RESET}");
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
        println!(
            "{ANSI_DIM}│{ANSI_RESET}  {color}{icon} {:<4} OK{ANSI_RESET}         : {:>5} pages {ANSI_DIM}({:.1}%){ANSI_RESET}",
            status, count, pct
        );
    }
    println!("{ANSI_DIM}└────────────────────────────────────────────────────────────────────────┘{ANSI_RESET}\n");

    // Top Priority Issues (Tree Format)
    println!("{ANSI_DIM}┌──[{ANSI_RESET} {ANSI_BOLD}DEFECT TRIAGE MATRIX{ANSI_RESET} {ANSI_DIM}]─────────────────────────────────────────────────{ANSI_RESET}");
    if result.issues.is_empty() {
        println!("{ANSI_DIM}│{ANSI_RESET}  {ANSI_GREEN}✔ Zero technical SEO defects detected across all probed nodes.{ANSI_RESET}");
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
            println!("    {ANSI_DIM}├── Defect  :{ANSI_RESET} {}", title);
            if !remediation.is_empty() {
                println!("    {ANSI_DIM}├── Remedy  :{ANSI_RESET} {}", remediation);
            }
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
    println!("{ANSI_DIM}┌──[{ANSI_RESET} {ANSI_BOLD}TOP AUTHORITY HUBS // PAGERANK DISTRIBUTION{ANSI_RESET} {ANSI_DIM}]─────────────────────────{ANSI_RESET}");
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
        println!("{ANSI_DIM}┌──[{ANSI_RESET} {ANSI_BOLD}GENERATED MISSION ARTIFACTS{ANSI_RESET} {ANSI_DIM}]─────────────────────────────────────────{ANSI_RESET}");
        for (fmt, path) in exported_paths {
            println!(
                "{ANSI_DIM}│{ANSI_RESET}  {ANSI_CYAN}◈ {:<9}{ANSI_RESET} : {}",
                fmt,
                path.display()
            );
        }
        println!("{ANSI_DIM}└────────────────────────────────────────────────────────────────────────┘{ANSI_RESET}\n");
    }
}
