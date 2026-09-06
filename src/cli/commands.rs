//! # CLI Command Handlers
//!
//! Implements execution logic for `audit`, `inspect`, `mcp`, `report`, and `list` commands.

use crate::cli::args::{AuditArgs, Cli, Commands, InspectArgs, ListArgs, McpArgs, ReportArgs};
use crate::core::config::CrawlConfig;
use crate::core::models::Severity;
use crate::crawler::engine::{run_crawl_with_options, CrawlResult, ProgressCallback};
use crate::crawler::inspector::inspect_url;
use crate::graph::{compute_pagerank, SiteGraph};
use crate::report::{
    create_crawl_progress_bar, export_json_report, export_markdown_report, finish_crawl_progress,
    print_audit_banner, print_executive_scorecard, print_historical_sessions,
    print_page_inspection, update_crawl_progress,
};
use crate::storage::{default_db_path, CrawlSessionInit, Database};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

/// Executes the parsed CLI command.
pub async fn execute(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
        Commands::Audit(args) => handle_audit(args).await,
        Commands::Inspect(args) => handle_inspect(args).await,
        Commands::Mcp(args) => handle_mcp(args).await,
        Commands::Report(args) => handle_report(args).await,
        Commands::List(args) => handle_list(args).await,
    }
}

/// Executes a full or partial website audit crawl.
async fn handle_audit(args: AuditArgs) -> Result<(), Box<dyn std::error::Error>> {
    let mut config = CrawlConfig::new(&args.url)?;
    config.max_pages = args.max_pages;
    config.max_depth = args.max_depth;
    config.concurrency = args.concurrency;
    config.delay_ms = args.delay;
    config.user_agent = args.user_agent;
    config.respect_robots = !args.no_robots;
    config.no_aimd = args.no_aimd;
    config.ephemeral = args.ephemeral;
    config.max_query_params = args.max_query_params;
    config.ignore_sorting_facets = args.ignore_sorting_facets;

    let db_path = args.db_path.clone().unwrap_or_else(default_db_path);
    config.db_path = Some(db_path.clone());

    let session_id = format!(
        "crawl_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    );
    config.session_id = Some(session_id.clone());

    let (writer_handle, writer_task) = if !args.ephemeral {
        let db = Database::open(&db_path)?;
        db.init_crawl_session(&CrawlSessionInit {
            session_id: session_id.clone(),
            target_url: config.start_url.clone(),
            max_pages: config.max_pages,
            max_depth: config.max_depth,
            respect_robots: config.respect_robots,
            render_js: config.render_js,
        })?;
        let (handle, task) = db.spawn_writer(&session_id, 50, Duration::from_millis(500))?;
        (Some(handle), Some(task))
    } else {
        (None, None)
    };

    print_audit_banner(
        &config.start_url,
        config.max_pages,
        config.concurrency,
        !config.no_aimd,
    );

    let pb = create_crawl_progress_bar(config.max_pages);
    let pb_clone = pb.clone();

    let progress_cb: ProgressCallback = Arc::new(move |update| {
        update_crawl_progress(&pb_clone, &update);
    });

    let crawl_result =
        run_crawl_with_options(&config, Some(progress_cb), writer_handle.clone(), None).await?;
    finish_crawl_progress(&pb);

    if let (Some(handle), Some(task)) = (writer_handle, writer_task) {
        let _ = handle.shutdown().await;
        let _ = task.await;
    }

    if args.ephemeral && db_path.exists() {
        let _ = std::fs::remove_file(&db_path);
        let mut shm = db_path.clone();
        shm.set_extension("db-shm");
        let _ = std::fs::remove_file(shm);
        let mut wal = db_path.clone();
        wal.set_extension("db-wal");
        let _ = std::fs::remove_file(wal);
    }

    // Handle file exports
    let formats: Vec<&str> = args.format.split(',').map(|s| s.trim()).collect();
    let mut exported_artifacts = Vec::new();

    if formats.contains(&"md") || formats.contains(&"all") {
        if let Ok(path) = export_markdown_report(&crawl_result, &args.output_dir) {
            exported_artifacts.push(("Markdown", path));
        }
    }

    if formats.contains(&"json") || formats.contains(&"all") {
        if let Ok(path) = export_json_report(&crawl_result, &args.output_dir) {
            exported_artifacts.push(("JSON", path));
        }
    }

    // Always display executive terminal scorecard if requested
    if formats.contains(&"terminal") || formats.contains(&"all") || formats.is_empty() {
        let ref_paths: Vec<(&str, &std::path::Path)> = exported_artifacts
            .iter()
            .map(|(fmt, p)| (*fmt, p.as_path()))
            .collect();
        print_executive_scorecard(&crawl_result, &ref_paths);
    }

    // Check CI/CD failure threshold
    let critical_count = crawl_result
        .issues
        .iter()
        .filter(|i| i.severity == Severity::Critical)
        .count();
    let alert_count = crawl_result
        .issues
        .iter()
        .filter(|i| i.severity == Severity::Alert)
        .count();
    let warning_count = crawl_result
        .issues
        .iter()
        .filter(|i| i.severity == Severity::Warning)
        .count();

    let should_fail = match args.fail_on.to_lowercase().as_str() {
        "critical" => critical_count > 0,
        "alert" => critical_count > 0 || alert_count > 0,
        "warning" => critical_count > 0 || alert_count > 0 || warning_count > 0,
        _ => false,
    };

    if should_fail {
        eprintln!(
            "❌ Audit failed CI/CD threshold policy (--fail-on {})",
            args.fail_on
        );
        std::process::exit(1);
    }

    Ok(())
}

/// Inspects a single page and prints its SEO metadata and audit issues.
async fn handle_inspect(args: InspectArgs) -> Result<(), Box<dyn std::error::Error>> {
    let timeout = Duration::from_secs(args.timeout);
    match inspect_url(&args.url, &args.user_agent, timeout).await {
        Ok((page, fetch, issues)) => {
            print_page_inspection(&page, &fetch, &issues);
            Ok(())
        }
        Err(err) => {
            eprintln!("❌ Failed to inspect URL '{}': {}", args.url, err);
            std::process::exit(2);
        }
    }
}

/// Launches the native Model Context Protocol (MCP) server.
async fn handle_mcp(args: McpArgs) -> Result<(), Box<dyn std::error::Error>> {
    info!(transport = %args.transport, "Starting MCP server (scaffold)");
    println!(
        "Starting SEO Lens MCP server on transport: {}",
        args.transport
    );
    Ok(())
}

/// Inspects or re-exports an existing audit session from persistence.
async fn handle_report(args: ReportArgs) -> Result<(), Box<dyn std::error::Error>> {
    let db_path = args.db_path.unwrap_or_else(default_db_path);
    if !db_path.exists() {
        eprintln!(
            "❌ Persistence database not found at '{}'. Run an audit first.",
            db_path.display()
        );
        std::process::exit(1);
    }

    let session_id = match args.session.or(args.session_pos) {
        Some(s) => s,
        None => {
            eprintln!("❌ Missing session ID. Usage: seolens report <SESSION_ID> or seolens report --session <SESSION_ID>");
            std::process::exit(1);
        }
    };

    let db = Database::open(&db_path)?;
    let crawl = match db.get_crawl(&session_id)? {
        Some(c) => c,
        None => {
            eprintln!(
                "❌ Session '{}' was not found in '{}'. Use 'seolens list' to see saved sessions.",
                session_id,
                db_path.display()
            );
            std::process::exit(1);
        }
    };

    println!("📦 Loading session '{}' from SQLite...", session_id);
    let pages = db.get_crawl_pages(&session_id, 100_000, 0)?;
    let issues = db.get_crawl_issues(&session_id, None, None)?;

    let sitemap_urls: Vec<String> = pages
        .iter()
        .filter(|p| p.is_sitemap_url)
        .map(|p| p.url.clone())
        .collect();
    let graph = SiteGraph::from_pages(&pages, &sitemap_urls);
    let pagerank = compute_pagerank(&graph, 0.85, 100, 1e-6);

    let crawl_result = CrawlResult {
        target_url: crawl.target_url.clone(),
        pages,
        graph,
        pagerank,
        issues,
        duration: Duration::from_secs(0),
        sitemap_urls,
        aimd_delay_ms: 0,
        health_score: crawl.health_score,
    };

    let output_dir = args
        .output_dir
        .unwrap_or_else(|| PathBuf::from("./reports"));
    let format_str = args
        .format
        .unwrap_or_else(|| "terminal,json,md".to_string());
    let formats: Vec<&str> = format_str.split(',').map(|s| s.trim()).collect();
    let mut exported_artifacts = Vec::new();

    if formats.contains(&"md") || formats.contains(&"all") {
        if let Ok(path) = export_markdown_report(&crawl_result, &output_dir) {
            exported_artifacts.push(("Markdown", path));
        }
    }

    if formats.contains(&"json") || formats.contains(&"all") {
        if let Ok(path) = export_json_report(&crawl_result, &output_dir) {
            exported_artifacts.push(("JSON", path));
        }
    }

    if formats.contains(&"terminal") || formats.contains(&"all") || formats.is_empty() {
        let ref_paths: Vec<(&str, &std::path::Path)> = exported_artifacts
            .iter()
            .map(|(fmt, p)| (*fmt, p.as_path()))
            .collect();
        print_executive_scorecard(&crawl_result, &ref_paths);
    }

    Ok(())
}

/// Lists historical audit sessions stored locally.
async fn handle_list(args: ListArgs) -> Result<(), Box<dyn std::error::Error>> {
    let db_path = args.db_path.unwrap_or_else(default_db_path);
    if !db_path.exists() {
        print_historical_sessions(&db_path, &[]);
        return Ok(());
    }

    let db = Database::open(&db_path)?;
    let crawls = db.list_crawls()?;

    print_historical_sessions(&db_path, &crawls);

    Ok(())
}
