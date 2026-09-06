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
    /// Drill down and filter audit findings for a session
    Issues(IssuesArgs),
    /// Check website readiness for AI search engines (ChatGPT Search, Perplexity, Claude) and /llms.txt
    CheckAi(CheckAiArgs),
    /// Delete a specific crawl session and its associated records
    Delete(DeleteArgs),
    /// Clean historical crawl sessions from the database
    Clean(CleanArgs),
    /// Validate JSON-LD / schema against Google Rich Results guidelines
    Schema(SchemaArgs),
}

/// Command-line arguments for the `list` subcommand.
#[derive(Args, Debug, Clone, Default)]
pub struct ListArgs {
    /// Maximum number of sessions to display
    #[arg(short = 'n', long, default_value_t = 20)]
    pub limit: usize,

    /// Output format: terminal (default) or json
    #[arg(short = 'f', long, default_value = "terminal")]
    pub format: String,

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

    /// Only crawl URLs matching this regex pattern
    #[arg(short = 'i', long)]
    pub include: Option<String>,

    /// Skip crawling URLs matching this regex pattern
    #[arg(short = 'e', long)]
    pub exclude: Option<String>,

    /// Custom HTTP request header(s) (e.g. -H "Authorization: Bearer xyz")
    #[arg(short = 'H', long = "header")]
    pub headers: Vec<String>,

    /// Explicit XML sitemap URL to crawl
    #[arg(long)]
    pub sitemap: Option<String>,

    /// Suppress progress bar output for clean CI/CD scripting
    #[arg(short = 'q', long, default_value_t = false)]
    pub quiet: bool,

    /// Optional audit or project name
    #[arg(long)]
    pub name: Option<String>,
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

    /// Output format: terminal (default), json, md
    #[arg(short = 'f', long, default_value = "terminal")]
    pub format: String,

    /// Custom HTTP request header(s) (e.g. -H "Authorization: Bearer xyz")
    #[arg(short = 'H', long = "header")]
    pub headers: Vec<String>,
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

/// Command-line arguments for the `issues` subcommand.
#[derive(Args, Debug, Clone, Default)]
pub struct IssuesArgs {
    /// Session ID to inspect
    #[arg(short = 's', long)]
    pub session: Option<String>,

    /// Session ID positional argument
    #[arg(value_name = "SESSION_ID")]
    pub session_pos: Option<String>,

    /// Filter by severity tier (critical, alert, warning, notice)
    #[arg(long)]
    pub severity: Option<String>,

    /// Filter by issue category (e.g. indexability, links, titles)
    #[arg(short = 'c', long)]
    pub category: Option<String>,

    /// Filter by rule ID code (e.g. ERR_HTTP_4XX_CLIENT_ERROR)
    #[arg(long)]
    pub code: Option<String>,

    /// Filter by URL substring (e.g. /blog/)
    #[arg(long)]
    pub url: Option<String>,

    /// Maximum number of issues to display
    #[arg(short = 'n', long, default_value_t = 50)]
    pub limit: usize,

    /// Pagination offset
    #[arg(long, default_value_t = 0)]
    pub offset: usize,

    /// Output format: terminal (default), json, md
    #[arg(short = 'f', long, default_value = "terminal")]
    pub format: String,

    /// Custom path to SQLite persistence database
    #[arg(long)]
    pub db_path: Option<PathBuf>,
}

/// Command-line arguments for the `check-ai` subcommand.
#[derive(Args, Debug, Clone)]
pub struct CheckAiArgs {
    /// Website root URL to check (e.g. https://example.com)
    pub url: String,

    /// Custom User-Agent string
    #[arg(short = 'u', long, default_value = "SEOLens/1.0")]
    pub user_agent: String,

    /// Request timeout in seconds
    #[arg(long, default_value_t = 15)]
    pub timeout: u64,

    /// Output format: terminal (default), json, md
    #[arg(short = 'f', long, default_value = "terminal")]
    pub format: String,
}

/// Command-line arguments for the `delete` subcommand.
#[derive(Args, Debug, Clone, Default)]
pub struct DeleteArgs {
    /// Session ID to delete
    #[arg(short = 's', long)]
    pub session: Option<String>,

    /// Session ID positional argument
    #[arg(value_name = "SESSION_ID")]
    pub session_pos: Option<String>,

    /// Custom path to SQLite persistence database
    #[arg(long)]
    pub db_path: Option<PathBuf>,
}

/// Command-line arguments for the `clean` subcommand.
#[derive(Args, Debug, Clone, Default)]
pub struct CleanArgs {
    /// Purge all crawl sessions
    #[arg(long, default_value_t = false)]
    pub all: bool,

    /// Purge sessions started older than N days ago
    #[arg(long)]
    pub older_than: Option<u32>,

    /// Custom path to SQLite persistence database
    #[arg(long)]
    pub db_path: Option<PathBuf>,
}

/// Command-line arguments for the `schema` subcommand.
#[derive(Args, Debug, Clone)]
pub struct SchemaArgs {
    /// URL or path to a local JSON/HTML file containing structured data
    pub target: String,

    /// Expected schema @type (e.g. Product, Article, FAQPage)
    #[arg(short = 't', long = "type")]
    pub expected_type: Option<String>,

    /// Output format: terminal (default) or json
    #[arg(short = 'f', long, default_value = "terminal")]
    pub format: String,

    /// Custom User-Agent string (when target is a URL)
    #[arg(short = 'u', long, default_value = "SEOLens/1.0")]
    pub user_agent: String,
}
