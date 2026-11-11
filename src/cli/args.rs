//! # Command-Line Argument Definitions
//!
//! Strongly-typed CLI options and subcommands for the `seolens` executable,
//! parsed via `clap`.

use clap::builder::styling::{AnsiColor, Effects, Styles};
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

/// Terminal styling for clap CLI outputs to match Black Sparrow pink/maroon palette.
pub fn cli_styles() -> Styles {
    Styles::styled()
        .header(AnsiColor::BrightMagenta.on_default() | Effects::BOLD)
        .usage(AnsiColor::BrightMagenta.on_default() | Effects::BOLD)
        .literal(AnsiColor::BrightMagenta.on_default() | Effects::BOLD)
        .placeholder(AnsiColor::Magenta.on_default())
        .valid(AnsiColor::BrightGreen.on_default())
        .invalid(AnsiColor::BrightRed.on_default())
}

/// Black Sparrow - High-performance website crawler & technical SEO audit engine
#[derive(Parser, Debug, Clone)]
#[command(
    name = env!("CARGO_PKG_NAME"),
    author,
    version,
    about = concat!(env!("CARGO_PKG_NAME"), " — High-performance website crawler & AI-native technical SEO audit engine"),
    styles = cli_styles()
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

/// Available CLI subcommands.
#[derive(Subcommand, Debug, Clone)]
pub enum Commands {
    /// Run full website crawl with AIMD adaptive rate limiting & technical audit
    Audit(AuditArgs),
    /// Instant developer X-ray for a single webpage (DOM, tags, headers)
    Inspect(InspectArgs),
    /// Start native Model Context Protocol server (stdio for Claude/Cursor)
    Mcp(McpArgs),
    /// Re-export or inspect an existing audit from the SQLite database
    Report(ReportArgs),
    /// List all historical audit sessions stored locally in SQLite
    List(ListArgs),
    /// Drill down and filter audit findings for a session
    Issues(IssuesArgs),
    /// Audit website readiness for AI search engines & /llms.txt
    CheckAi(CheckAiArgs),
    /// Delete a specific crawl session and its associated records
    Delete(DeleteArgs),
    /// Clean historical crawl sessions from the database
    Clean(CleanArgs),
    /// Validate JSON-LD / schema against Google Rich Results guidelines
    Schema(SchemaArgs),
    /// Extract a business website's content, contacts and design as JSON (brand kits, AI agents)
    Extract(ExtractArgs),
}

/// Command-line arguments for the `extract` subcommand.
#[derive(Args, Debug, Clone)]
pub struct ExtractArgs {
    /// Website to read (e.g. `https://example.com`)
    pub url: String,

    /// Pages to read in total, homepage included
    #[arg(short = 'p', long, default_value_t = 6)]
    pub max_pages: usize,

    /// Read these pages after the homepage instead of choosing them by rules (repeatable)
    #[arg(long = "page", value_name = "URL")]
    pub pages: Vec<String>,

    /// Most stylesheets fetched for the design analysis
    #[arg(long, default_value_t = 10)]
    pub max_stylesheets: usize,

    /// When to render pages in headless Chromium: auto (JavaScript shells and bot challenges), always, never
    #[arg(long, default_value = "auto", value_parser = ["auto", "always", "never"])]
    pub render: String,

    /// Chromium executable used for rendering (default: `CHROME_PATH`, then the `PATH`)
    #[arg(long, value_name = "PATH")]
    pub chrome_path: Option<PathBuf>,

    /// DevTools WebSocket of a running Chromium to render with instead of launching one
    #[arg(long, value_name = "WS_URL")]
    pub chrome_ws: Option<String>,

    /// Extra switch passed to a launched Chromium (repeatable)
    #[arg(long = "chrome-arg", value_name = "ARG", allow_hyphen_values = true)]
    pub chrome_args: Vec<String>,

    /// Run Chromium without its sandbox (containers without user namespaces)
    #[arg(long, default_value_t = false)]
    pub chrome_no_sandbox: bool,

    /// Allow private and loopback addresses (local test sites only)
    #[arg(long, default_value_t = false)]
    pub allow_private_network: bool,

    /// Send all traffic through this egress proxy (`http://[user:pass@]host:port`); proxy
    /// environment variables are ignored. The proxy resolves names, so it must enforce its own
    /// egress rules.
    #[arg(long, value_name = "URL")]
    pub proxy: Option<String>,

    /// Longest the whole extraction may take, in seconds
    #[arg(long, default_value_t = 90)]
    pub timeout_secs: u64,

    /// Longest one HTTP request may take, in seconds
    #[arg(long, default_value_t = 20)]
    pub request_timeout_secs: u64,

    /// Custom User-Agent string
    #[arg(short = 'u', long)]
    pub user_agent: Option<String>,

    /// Pretty-print the JSON
    #[arg(long, default_value_t = false)]
    pub pretty: bool,

    /// Write the JSON to this file instead of stdout
    #[arg(short = 'o', long, value_name = "FILE")]
    pub output: Option<PathBuf>,
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

    /// Force database persistence to project-local `.seolens/seolens.db`
    #[arg(short = 'L', long, default_value_t = false)]
    pub local: bool,
}

/// Command-line arguments for the `audit` subcommand.
#[derive(Args, Debug, Clone)]
pub struct AuditArgs {
    /// Root URL to crawl (e.g. `https://example.com`)
    pub url: String,

    /// Maximum pages to crawl (0 = unlimited)
    #[arg(short = 'p', long, default_value_t = 500)]
    pub max_pages: u32,

    /// Maximum crawl depth from start URL (0 = unlimited)
    #[arg(short = 'd', long)]
    pub max_depth: Option<u16>,

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

    /// Chrome executable for `--render-js` (default: `CHROME_PATH`, then the `PATH`)
    #[arg(long, value_name = "PATH")]
    pub chrome_path: Option<PathBuf>,

    /// Run Chrome without its sandbox (containers, root)
    #[arg(long, default_value_t = false)]
    pub chrome_no_sandbox: bool,

    /// Custom User-Agent string
    #[arg(short = 'u', long, default_value = "BlackSparrow/1.0")]
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

    /// Custom path to SQLite persistence database
    #[arg(long)]
    pub db_path: Option<PathBuf>,

    /// Force database persistence to project-local `.seolens/seolens.db`
    #[arg(short = 'L', long, default_value_t = false)]
    pub local: bool,

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

    /// Specific private host or host:port allowed to be crawled (can be repeated, e.g. -a localhost:3000 or --allow-host localhost:3000)
    #[arg(
        short = 'a',
        long = "allow-host",
        visible_alias = "allow",
        value_name = "HOST"
    )]
    pub allowed_hosts: Vec<String>,
}

impl Default for AuditArgs {
    fn default() -> Self {
        Self {
            url: String::new(),
            max_pages: 500,
            max_depth: None,
            concurrency: 10,
            delay: 0,
            render_js: false,
            chrome_ws: "auto".to_string(),
            chrome_path: None,
            chrome_no_sandbox: false,
            user_agent: crate::core::branding::DEFAULT_USER_AGENT.to_string(),
            format: "terminal,json,md".to_string(),
            output_dir: PathBuf::from("./reports"),
            fail_on: "none".to_string(),
            no_robots: false,
            ephemeral: false,
            no_aimd: false,
            max_query_params: 2,
            ignore_sorting_facets: true,
            db_path: None,
            local: false,
            include: None,
            exclude: None,
            headers: Vec::new(),
            sitemap: None,
            quiet: false,
            name: None,
            allowed_hosts: Vec::new(),
        }
    }
}

/// Command-line arguments for the `inspect` subcommand.
#[derive(Args, Debug, Clone)]
pub struct InspectArgs {
    /// URL of the page to inspect (e.g. `https://example.com/blog/my-post`)
    pub url: String,

    /// Custom User-Agent string
    #[arg(short = 'u', long, default_value = "BlackSparrow/1.0")]
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

    /// Specific private host or host:port allowed to be inspected (can be repeated, e.g. -a localhost:3000 or --allow-host localhost:3000)
    #[arg(
        short = 'a',
        long = "allow-host",
        visible_alias = "allow",
        value_name = "HOST"
    )]
    pub allowed_hosts: Vec<String>,
}

/// Command-line arguments for the `mcp` subcommand.
#[derive(Args, Debug, Clone, Default)]
pub struct McpArgs {
    /// Transport mechanism: stdio (local agents) or sse (remote HTTP)
    #[arg(long, default_value = "stdio")]
    pub transport: String,

    /// Port to bind for HTTP/SSE transport (when --transport sse)
    #[arg(long, default_value_t = 8080)]
    pub port: u16,

    /// Custom path to SQLite persistence database
    #[arg(long)]
    pub db_path: Option<PathBuf>,

    /// Force database persistence to project-local `.seolens/seolens.db`
    #[arg(short = 'L', long, default_value_t = false)]
    pub local: bool,

    /// Allow auditing local and private network addresses (default: false). Cloud metadata endpoints remain permanently blocked.
    #[arg(long, default_value_t = false)]
    pub allow_local_network: bool,

    /// Specific private host or host:port allowed for MCP tools (can be repeated, e.g. -a localhost:3000 or --allow-host localhost:3000)
    #[arg(
        short = 'a',
        long = "allow-host",
        visible_alias = "allow",
        value_name = "HOST"
    )]
    pub allowed_hosts: Vec<String>,
}

/// Command-line arguments for the `report` subcommand.
#[derive(Args, Debug, Clone, Default)]
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

    /// Force database persistence to project-local `.seolens/seolens.db`
    #[arg(short = 'L', long, default_value_t = false)]
    pub local: bool,
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

    /// Force database persistence to project-local `.seolens/seolens.db`
    #[arg(short = 'L', long, default_value_t = false)]
    pub local: bool,
}

/// Command-line arguments for the `check-ai` subcommand.
#[derive(Args, Debug, Clone)]
pub struct CheckAiArgs {
    /// Website root URL to check (e.g. https://example.com)
    pub url: String,

    /// Custom User-Agent string
    #[arg(short = 'u', long, default_value = "BlackSparrow/1.0")]
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

    /// Force database persistence to project-local `.seolens/seolens.db`
    #[arg(short = 'L', long, default_value_t = false)]
    pub local: bool,
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

    /// Force database persistence to project-local `.seolens/seolens.db`
    #[arg(short = 'L', long, default_value_t = false)]
    pub local: bool,
}

/// Command-line arguments for the `schema` subcommand.
#[derive(Args, Debug, Clone)]
#[command(subcommand_negates_reqs = true, args_conflicts_with_subcommands = true)]
pub struct SchemaArgs {
    /// Print a JSON Schema instead of validating structured data
    #[command(subcommand)]
    pub document: Option<SchemaDocument>,

    /// URL or path to a local JSON/HTML file containing structured data
    #[arg(required = true)]
    pub target: Option<String>,

    /// Expected schema @type (e.g. Product, Article, FAQPage)
    #[arg(short = 't', long = "type")]
    pub expected_type: Option<String>,

    /// Output format: terminal (default) or json
    #[arg(short = 'f', long, default_value = "terminal")]
    pub format: String,

    /// Custom User-Agent string (when target is a URL)
    #[arg(short = 'u', long, default_value = "BlackSparrow/1.0")]
    pub user_agent: String,
}

/// Documents whose JSON Schema `schema` can print.
#[derive(Subcommand, Debug, Clone)]
pub enum SchemaDocument {
    /// Print the JSON Schema (draft 2020-12) of the `extract` output document
    Extract,
}
