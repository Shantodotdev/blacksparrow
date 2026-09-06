//! # SEO Lens CLI Entry Point
//!
//! Command-line interface for the `seolens` binary.
//!
//! ## Subcommands
//!
//! - `audit`: Executes a full or partial website crawl and audits pages against technical SEO rules.
//! - `mcp`: Launches the native Model Context Protocol (MCP) server over `stdio` or HTTP/SSE.
//! - `report`: Inspects, filters, and re-exports historical crawl sessions from SQLite.
//! - `list`: Displays a summary table of past audit sessions stored locally.
//!
//! ## Usage Examples
//!
//! ```bash
//! # Run a standard audit crawl with 20 concurrent tasks
//! seolens audit https://example.com -p 500 -d 5 -c 20
//!
//! # Run with Headless Chrome JavaScript rendering enabled
//! seolens audit https://example.com --render-js
//!
//! # Run MCP server for Cursor or Claude Desktop integration
//! seolens mcp --transport stdio
//!
//! # Export audit results to Markdown and JSON reports
//! seolens report --session 20260904_183012_abc -f md,json -o ./reports
//! ```

use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;
use tracing::info;
use tracing_subscriber::EnvFilter;

/// SEO Lens - High-performance website crawler & technical SEO audit engine
#[derive(Parser, Debug)]
#[command(name = "seolens", author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Run a full or partial website crawl and audit
    Audit(AuditArgs),
    /// Inspect a single webpage: metadata, Open Graph, Twitter cards, JSON-LD, headings, and audit issues
    Inspect(InspectArgs),
    /// Start the native Model Context Protocol server (stdio for Cursor/Claude)
    Mcp(McpArgs),
    /// Re-export or inspect an existing audit from the SQLite database
    Report(ReportArgs),
    /// List all historical audit sessions stored locally
    List,
}

#[derive(Args, Debug)]
pub struct AuditArgs {
    /// Root URL to crawl (e.g. `https://example.com`)
    pub url: String,

    /// Maximum pages to crawl (0 = unlimited)
    #[arg(short = 'p', long, default_value_t = 500)]
    pub max_pages: u32,

    /// Maximum crawl depth from start URL
    #[arg(short = 'd', long, default_value_t = 5)]
    pub max_depth: u16,

    /// Number of concurrent fetch tasks
    #[arg(short = 'c', long, default_value_t = 10)]
    pub concurrency: usize,

    /// Delay between requests in milliseconds (0 = auto-AIMD)
    #[arg(long, default_value_t = 0)]
    pub delay: u64,

    /// Enable Headless Chrome CDP for JavaScript rendering
    #[arg(long, default_value_t = false)]
    pub render_js: bool,

    /// Remote Chrome WebSocket URL (e.g. `ws://127.0.0.1:9222`)
    #[arg(long, default_value = "auto")]
    pub chrome_ws: String,

    /// Custom User-Agent string
    #[arg(short = 'u', long, default_value = "SEOLens/1.0")]
    pub user_agent: String,

    /// Comma-separated outputs: terminal,json,md,html,csv,all
    #[arg(short = 'f', long, default_value = "terminal,json,md")]
    pub format: String,

    /// Directory where export artifacts are saved
    #[arg(short = 'o', long, default_value = "./reports")]
    pub output_dir: PathBuf,

    /// CI/CD threshold: critical, alert, or warning. Returns exit code 1 if matched
    #[arg(long, default_value = "none")]
    pub fail_on: String,

    /// Ignore /robots.txt disallow rules
    #[arg(long, default_value_t = false)]
    pub no_robots: bool,

    /// Do not persist results to SQLite; auto-cleanup on finish
    #[arg(long, default_value_t = false)]
    pub ephemeral: bool,

    /// Disable dynamic AIMD rate throttling (useful for high-speed local site crawls)
    #[arg(long, default_value_t = false)]
    pub no_aimd: bool,

    /// Maximum number of content query parameters before flagging or pruning faceted spider traps
    #[arg(long, default_value_t = 2)]
    pub max_query_params: usize,

    /// Strip/prune sorting and display facets (sort, order, view, etc.)
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    pub ignore_sorting_facets: bool,
}

#[derive(Args, Debug)]
pub struct InspectArgs {
    /// URL of the page to inspect (e.g. `https://example.com/blog/my-post`)
    pub url: String,

    /// Custom User-Agent string
    #[arg(short = 'u', long, default_value = "SEOLens/1.0")]
    pub user_agent: String,

    /// Request timeout in seconds
    #[arg(long, default_value_t = 15)]
    pub timeout: u64,
}

#[derive(Args, Debug)]
pub struct McpArgs {
    /// Transport mechanism: stdio (local agents) or sse (remote HTTP)
    #[arg(long, default_value = "stdio")]
    pub transport: String,

    /// Port to bind for HTTP/SSE transport (when --transport sse)
    #[arg(long, default_value_t = 8080)]
    pub port: u16,
}

#[derive(Args, Debug)]
pub struct ReportArgs {
    /// Session ID to inspect or re-export
    pub session: String,

    /// Comma-separated export formats: csv,json,md,html
    #[arg(short = 'f', long)]
    pub format: Option<String>,

    /// Directory where export artifacts are saved
    #[arg(short = 'o', long)]
    pub output_dir: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging subscriber (quiet by default unless RUST_LOG is explicitly provided)
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Audit(args) => {
            let mut config = seo_lens::core::config::CrawlConfig::new(&args.url)?;
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

            seo_lens::report::print_audit_banner(
                &config.start_url,
                config.max_pages,
                config.concurrency,
                !config.no_aimd,
            );

            let pb = seo_lens::report::create_crawl_progress_bar(config.max_pages);
            let pb_clone = pb.clone();

            let progress_cb: seo_lens::crawler::ProgressCallback =
                std::sync::Arc::new(move |update| {
                    seo_lens::report::update_crawl_progress(&pb_clone, &update);
                });

            let crawl_result = seo_lens::crawler::run_crawl(&config, Some(progress_cb)).await?;
            seo_lens::report::finish_crawl_progress(&pb);

            // Handle file exports
            let formats: Vec<&str> = args.format.split(',').map(|s| s.trim()).collect();
            let mut exported_artifacts = Vec::new();

            if formats.contains(&"md") || formats.contains(&"all") {
                if let Ok(path) =
                    seo_lens::report::export_markdown_report(&crawl_result, &args.output_dir)
                {
                    exported_artifacts.push(("Markdown", path));
                }
            }

            if formats.contains(&"json") || formats.contains(&"all") {
                if let Ok(path) =
                    seo_lens::report::export_json_report(&crawl_result, &args.output_dir)
                {
                    exported_artifacts.push(("JSON", path));
                }
            }

            // Always display executive terminal scorecard if requested
            if formats.contains(&"terminal") || formats.contains(&"all") || formats.is_empty() {
                let ref_paths: Vec<(&str, &std::path::Path)> = exported_artifacts
                    .iter()
                    .map(|(fmt, p)| (*fmt, p.as_path()))
                    .collect();
                seo_lens::report::print_executive_scorecard(&crawl_result, &ref_paths);
            }

            // Check CI/CD failure threshold
            let critical_count = crawl_result
                .issues
                .iter()
                .filter(|i| i.severity == seo_lens::core::models::Severity::Critical)
                .count();
            let alert_count = crawl_result
                .issues
                .iter()
                .filter(|i| i.severity == seo_lens::core::models::Severity::Alert)
                .count();
            let warning_count = crawl_result
                .issues
                .iter()
                .filter(|i| i.severity == seo_lens::core::models::Severity::Warning)
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
        }

        Commands::Inspect(args) => {
            let timeout = std::time::Duration::from_secs(args.timeout);
            match seo_lens::crawler::inspect_url(&args.url, &args.user_agent, timeout).await {
                Ok((page, fetch, issues)) => {
                    seo_lens::report::print_page_inspection(&page, &fetch, &issues);
                }
                Err(err) => {
                    eprintln!("❌ Failed to inspect URL '{}': {}", args.url, err);
                    std::process::exit(2);
                }
            }
        }

        Commands::Mcp(args) => {
            info!(transport = %args.transport, "Starting MCP server (scaffold)");
            println!(
                "Starting SEO Lens MCP server on transport: {}",
                args.transport
            );
        }
        Commands::Report(args) => {
            info!(session_id = %args.session, "Inspecting report session (scaffold)");
            println!("Exporting audit session: {}", args.session);
        }
        Commands::List => {
            info!("Listing audit sessions (scaffold)");
            println!("Stored audit sessions: none (Phase 0 scaffolding)");
        }
    }

    Ok(())
}
