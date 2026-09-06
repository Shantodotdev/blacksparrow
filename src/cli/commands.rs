//! # CLI Command Handlers
//!
//! Implements execution logic for `audit`, `inspect`, `mcp`, `report`, and `list` commands.

use crate::cli::args::{AuditArgs, Cli, Commands, InspectArgs, McpArgs, ReportArgs};
use crate::core::config::CrawlConfig;
use crate::core::models::Severity;
use crate::crawler::engine::{run_crawl, ProgressCallback};
use crate::crawler::inspector::inspect_url;
use crate::report::{
    create_crawl_progress_bar, export_json_report, export_markdown_report, finish_crawl_progress,
    print_audit_banner, print_executive_scorecard, print_page_inspection, update_crawl_progress,
};
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
        Commands::List => handle_list().await,
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

    let crawl_result = run_crawl(&config, Some(progress_cb)).await?;
    finish_crawl_progress(&pb);

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
    info!(session_id = %args.session, "Inspecting report session (scaffold)");
    println!("Exporting audit session: {}", args.session);
    Ok(())
}

/// Lists historical audit sessions stored locally.
async fn handle_list() -> Result<(), Box<dyn std::error::Error>> {
    info!("Listing audit sessions (scaffold)");
    println!("Stored audit sessions: none (Phase 0 scaffolding)");
    Ok(())
}
