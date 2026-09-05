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
    // Initialize logging subscriber
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Audit(args) => {
            let normalized_url = seo_lens::core::url::normalize_url(&args.url)?;
            info!(target_url = %normalized_url, "Initiating live audit fetch");
            println!("🔍 Fetching & Auditing {}...", normalized_url);

            let client = seo_lens::crawler::client::HttpClient::new(
                seo_lens::crawler::client::FetchOptions {
                    user_agent: args.user_agent,
                    timeout: std::time::Duration::from_secs(30),
                    max_redirects: 10,
                    ..Default::default()
                },
            )?;

            let fetch_res = client.fetch(&normalized_url).await?;
            println!("\n=== HTTP TRANSPORT TELEMETRY ===");
            println!("• Status Code   : {}", fetch_res.status_code);
            println!("• Final URL     : {}", fetch_res.final_url);
            println!("• TTFB Latency  : {} ms", fetch_res.ttfb_ms);
            println!("• Payload Size  : {} bytes", fetch_res.size_bytes);
            println!("• Content-Type  : {}", fetch_res.content_type);
            if !fetch_res.redirect_chain.is_empty() {
                println!(
                    "• Redirect Chain: {}",
                    fetch_res.redirect_chain.join(" -> ")
                );
            }
            if let Some(waf) = fetch_res.waf_detected {
                println!("⚠️  WAF Challenge Detected: {}", waf);
            }

            // Stream-parse HTML
            let parsed = seo_lens::parser::parse_html(&fetch_res.body, &fetch_res.final_url)?;
            println!("\n=== EXTRACTED SEO METADATA ===");
            println!(
                "• Document Title: {}",
                parsed.title.as_deref().unwrap_or("[MISSING]")
            );
            println!(
                "• Meta Desc     : {}",
                parsed.meta_description.as_deref().unwrap_or("[MISSING]")
            );
            println!(
                "• Canonical URL : {}",
                parsed.canonical_url.as_deref().unwrap_or("[MISSING]")
            );
            println!(
                "• Primary H1    : {}",
                parsed.h1_primary.as_deref().unwrap_or("[MISSING]")
            );
            println!("• Total H1 Count: {}", parsed.h1_count);
            println!("• H2 Headings   : {}", parsed.h2_headings.len());
            println!("• H3 Headings   : {}", parsed.h3_headings.len());
            println!("• Word Count    : {} words", parsed.word_count);
            println!("• SimHash       : {:016x}", parsed.simhash);
            println!("• Content Hash  : {:016x}", parsed.content_hash);
            println!("• Robots Direct : {:?}", parsed.robots_flags);
            println!(
                "• Internal Links: {}",
                parsed.links.iter().filter(|l| l.is_internal).count()
            );
            println!(
                "• Outbound Links: {}",
                parsed.links.iter().filter(|l| !l.is_internal).count()
            );
            println!("• Images Found  : {}", parsed.images.len());
            println!("• Schemas (JSON): {}", parsed.schemas.len());
            println!("\n✨ Live audit check completed successfully!");
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
