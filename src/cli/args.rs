//! # Command-Line Argument Definitions
//!
//! Strongly-typed CLI options and subcommands for the `seolens` executable,
//! parsed via `clap`.

use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

/// SEO Lens - High-performance website crawler & technical SEO audit engine
#[derive(Parser, Debug, Clone)]
#[command(name = "seolens", author, version, about, long_about = None)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

/// Available CLI subcommands.
#[derive(Subcommand, Debug, Clone)]
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
    List(ListArgs),
}

/// Command-line arguments for the `list` subcommand.
#[derive(Args, Debug, Clone, Default)]
pub struct ListArgs {
    /// Custom path to SQLite persistence database
    #[arg(long)]
    pub db_path: Option<PathBuf>,
}

/// Command-line arguments for the `audit` subcommand.
#[derive(Args, Debug, Clone)]
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

    /// Custom path to SQLite persistence database (default: .seolens/seolens.db)
    #[arg(long)]
    pub db_path: Option<PathBuf>,
}

/// Command-line arguments for the `inspect` subcommand.
#[derive(Args, Debug, Clone)]
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

/// Command-line arguments for the `mcp` subcommand.
#[derive(Args, Debug, Clone)]
pub struct McpArgs {
    /// Transport mechanism: stdio (local agents) or sse (remote HTTP)
    #[arg(long, default_value = "stdio")]
    pub transport: String,

    /// Port to bind for HTTP/SSE transport (when --transport sse)
    #[arg(long, default_value_t = 8080)]
    pub port: u16,
}

/// Command-line arguments for the `report` subcommand.
#[derive(Args, Debug, Clone)]
pub struct ReportArgs {
    /// Session ID to inspect or re-export
    #[arg(short = 's', long)]
    pub session: Option<String>,

    /// Session ID positional argument
    #[arg(value_name = "SESSION_ID")]
    pub session_pos: Option<String>,

    /// Comma-separated export formats: csv,json,md,html
    #[arg(short = 'f', long)]
    pub format: Option<String>,

    /// Directory where export artifacts are saved
    #[arg(short = 'o', long)]
    pub output_dir: Option<PathBuf>,

    /// Custom path to SQLite persistence database
    #[arg(long)]
    pub db_path: Option<PathBuf>,
}
