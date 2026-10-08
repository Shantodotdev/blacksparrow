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
    /// Fetch one page as clean Markdown for AI agents
    Scrape(ScrapeArgs),
    /// List a site's URLs from links, robots.txt and sitemaps
    Map(MapArgs),
    /// Scrape many pages of a site into Markdown files or NDJSON
    Crawl(CrawlArgs),
    /// Find passages by question, CSS selector or regex
    Find(FindArgs),
    /// Extract schema-shaped JSON from pages without an LLM
    Extract(ExtractArgs),
    /// Run browser steps (click, type, scroll) and read the page
    Interact(InteractArgs),
    /// Serve the Firecrawl-compatible HTTP API
    #[cfg(feature = "serve")]
    Serve(ServeArgs),
}

/// Network, identity and storage flags shared by the agent commands.
#[derive(Args, Debug, Clone, Default)]
pub struct WebArgs {
    /// Custom User-Agent string
    #[arg(short = 'u', long)]
    pub user_agent: Option<String>,

    /// Custom HTTP request header(s) (e.g. -H "Authorization: Bearer xyz")
    #[arg(short = 'H', long = "header")]
    pub headers: Vec<String>,

    /// Specific private host or host:port that may be fetched (can be repeated)
    #[arg(short = 'a', long = "allow-host", value_name = "HOST")]
    pub allowed_hosts: Vec<String>,

    /// Allow local and private network addresses (cloud metadata stays blocked)
    #[arg(long, default_value_t = false)]
    pub allow_local_network: bool,

    /// Remote Chrome WebSocket URL (default: launch a local Chrome when needed)
    #[arg(long)]
    pub chrome_ws: Option<String>,

    /// Maximum Chrome tabs rendering at once
    #[arg(long, default_value_t = 4)]
    pub render_concurrency: usize,

    /// Ignore /robots.txt disallow rules
    #[arg(long, default_value_t = false)]
    pub no_robots: bool,

    /// Per-request timeout in seconds
    #[arg(long, default_value_t = 30)]
    pub timeout: u64,

    /// Custom path to SQLite persistence database
    #[arg(long)]
    pub db_path: Option<PathBuf>,

    /// Force database persistence to project-local `.seolens/seolens.db`
    #[arg(short = 'L', long, default_value_t = false)]
    pub local: bool,

    /// Do not store pages (no cache, change tracking or crawl-wide find)
    #[arg(long, default_value_t = false)]
    pub no_store: bool,
}

/// Page conversion flags shared by `scrape` and `crawl`.
#[derive(Args, Debug, Clone, Default)]
pub struct PageArgs {
    /// Comma-separated outputs: markdown,json,text,links,metadata,html,raw_html,screenshot
    #[arg(short = 'f', long, default_value = "markdown")]
    pub format: String,

    /// Keep navigation, headers and footers (default: main content only)
    #[arg(long, default_value_t = false)]
    pub full_page: bool,

    /// Chrome policy: auto (only empty app shells), never, always
    #[arg(long, default_value = "auto")]
    pub render: String,

    /// CSS selector to wait for before reading (renders the page)
    #[arg(long)]
    pub wait_for: Option<String>,

    /// CSS selector(s) whose elements form the content
    #[arg(long = "include-selector")]
    pub include_selectors: Vec<String>,

    /// CSS selector(s) removed before extraction
    #[arg(long = "exclude-selector")]
    pub exclude_selectors: Vec<String>,

    /// Trim Markdown to about this many tokens, keeping every heading
    #[arg(long)]
    pub max_tokens: Option<usize>,

    /// Reuse a stored copy younger than this many seconds
    #[arg(long)]
    pub max_age: Option<u64>,
}

/// Command-line arguments for the `scrape` subcommand.
#[derive(Args, Debug, Clone)]
pub struct ScrapeArgs {
    /// Page URL
    pub url: String,

    #[command(flatten)]
    pub page: PageArgs,

    /// Print the whole document as JSON instead of Markdown
    #[arg(long, default_value_t = false)]
    pub json: bool,

    /// Write the output to this file instead of stdout
    #[arg(short = 'o', long)]
    pub output: Option<PathBuf>,

    #[command(flatten)]
    pub web: WebArgs,
}

/// Command-line arguments for the `map` subcommand.
#[derive(Args, Debug, Clone)]
pub struct MapArgs {
    /// Site start URL
    pub url: String,

    /// Rank URLs by relevance to these words
    #[arg(short = 's', long)]
    pub search: Option<String>,

    /// Maximum URLs returned
    #[arg(short = 'n', long, default_value_t = 5000)]
    pub limit: usize,

    /// Only keep paths matching these glob or regex patterns
    #[arg(long = "include-path")]
    pub include_paths: Vec<String>,

    /// Drop paths matching these glob or regex patterns
    #[arg(long = "exclude-path")]
    pub exclude_paths: Vec<String>,

    /// Sitemap usage: include, skip or only
    #[arg(long, default_value = "include")]
    pub sitemap: String,

    /// Keep URLs on subdomains of the start host
    #[arg(long, default_value_t = false)]
    pub subdomains: bool,

    /// Print JSON (URL, title, source, score) instead of one URL per line
    #[arg(long, default_value_t = false)]
    pub json: bool,

    #[command(flatten)]
    pub web: WebArgs,
}

/// Command-line arguments for the `crawl` subcommand.
#[derive(Args, Debug, Clone)]
pub struct CrawlArgs {
    /// Start URL
    pub url: String,

    /// Maximum pages scraped
    #[arg(short = 'n', long, default_value_t = 100)]
    pub limit: usize,

    /// Maximum link depth from the start URL
    #[arg(short = 'd', long, default_value_t = 5)]
    pub max_depth: u16,

    /// Only scrape paths matching these glob or regex patterns
    #[arg(long = "include-path")]
    pub include_paths: Vec<String>,

    /// Never scrape paths matching these glob or regex patterns
    #[arg(long = "exclude-path")]
    pub exclude_paths: Vec<String>,

    /// Sitemap usage: include, skip or only
    #[arg(long, default_value = "include")]
    pub sitemap: String,

    /// Follow links to subdomains of the start host
    #[arg(long, default_value_t = false)]
    pub subdomains: bool,

    /// Pages fetched at once
    #[arg(short = 'c', long, default_value_t = 4)]
    pub concurrency: usize,

    /// Fixed delay between requests in milliseconds (0 = adaptive)
    #[arg(long, default_value_t = 0)]
    pub delay: u64,

    /// Keep text repeated on most pages (navigation, banners)
    #[arg(long, default_value_t = false)]
    pub keep_boilerplate: bool,

    /// Write one Markdown file per page under this directory (default: NDJSON to stdout)
    #[arg(short = 'o', long)]
    pub out: Option<PathBuf>,

    #[command(flatten)]
    pub page: PageArgs,

    #[command(flatten)]
    pub web: WebArgs,
}

/// Command-line arguments for the `find` subcommand.
#[derive(Args, Debug, Clone)]
pub struct FindArgs {
    /// Page to search (omit with --crawl)
    pub url: Option<String>,

    /// Search the stored pages of this crawl id
    #[arg(long)]
    pub crawl: Option<String>,

    /// Restrict stored pages to URLs starting with this prefix
    #[arg(long)]
    pub url_prefix: Option<String>,

    /// Question or keywords
    #[arg(short = 'q', long)]
    pub query: Option<String>,

    /// CSS selector (page only)
    #[arg(short = 's', long)]
    pub selector: Option<String>,

    /// Regular expression
    #[arg(short = 'r', long)]
    pub regex: Option<String>,

    /// Passages returned for a query
    #[arg(short = 'k', long, default_value_t = 5)]
    pub top_k: usize,

    /// Attribute(s) returned for selector matches (e.g. --attr href)
    #[arg(long = "attr")]
    pub attributes: Vec<String>,

    #[command(flatten)]
    pub web: WebArgs,
}

/// Command-line arguments for the `extract` subcommand.
#[derive(Args, Debug, Clone)]
pub struct ExtractArgs {
    /// Page URL(s)
    pub urls: Vec<String>,

    /// Extract from every stored page of this crawl id
    #[arg(long)]
    pub crawl: Option<String>,

    /// JSON schema: a file path or inline JSON
    #[arg(long)]
    pub schema: String,

    /// Selector rules ({"base": …, "fields": {…}}): a file path or inline JSON
    #[arg(long)]
    pub rules: Option<String>,

    /// Fields below this confidence are reported as low confidence
    #[arg(long, default_value_t = 0.6)]
    pub min_confidence: f64,

    /// Do not learn selectors from pages with structured data
    #[arg(long, default_value_t = false)]
    pub no_learn: bool,

    /// Pages processed at most
    #[arg(short = 'n', long, default_value_t = 100)]
    pub limit: usize,

    #[command(flatten)]
    pub web: WebArgs,
}

/// Command-line arguments for the `interact` subcommand.
#[derive(Args, Debug, Clone)]
pub struct InteractArgs {
    /// Page URL
    pub url: String,

    /// Steps as a JSON array (file path or inline), e.g. '[{"type":"click","target":"e3"}]'
    #[arg(long)]
    pub steps: Option<String>,

    /// Selector that must appear before the first step
    #[arg(long)]
    pub wait_for: Option<String>,

    /// Save a full-page PNG screenshot to this file
    #[arg(long)]
    pub screenshot: Option<PathBuf>,

    /// Print the whole result as JSON
    #[arg(long, default_value_t = false)]
    pub json: bool,

    #[command(flatten)]
    pub web: WebArgs,
}

/// Command-line arguments for the `serve` subcommand.
#[cfg(feature = "serve")]
#[derive(Args, Debug, Clone)]
pub struct ServeArgs {
    /// Address to listen on (non-loopback addresses require API keys)
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    /// Port to listen on
    #[arg(short = 'p', long, default_value_t = 3002)]
    pub port: u16,

    /// Accepted API key(s); also read from BLACKSPARROW_API_KEYS (comma-separated)
    #[arg(long = "api-key")]
    pub api_keys: Vec<String>,

    /// Requests per key per minute (0 = unlimited)
    #[arg(long, default_value_t = 120)]
    pub rate_limit: u32,

    /// Most pages one crawl or extract request may process
    #[arg(long, default_value_t = 1000)]
    pub max_crawl_pages: usize,

    /// Most crawls running at once
    #[arg(long, default_value_t = 4)]
    pub max_concurrent_crawls: usize,

    /// Largest accepted request body in bytes
    #[arg(long, default_value_t = 1024 * 1024)]
    pub max_body_bytes: usize,

    /// External base URL used in crawl status links (e.g. https://crawl.example.com)
    #[arg(long)]
    pub public_url: Option<String>,

    #[command(flatten)]
    pub web: WebArgs,
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

    /// Tool families to expose: seo (audit tools), web (scrape, crawl, find, extract), or all
    #[arg(long, default_value = "all")]
    pub tools: String,

    /// Remote Chrome WebSocket URL for the web tools
    #[arg(long)]
    pub chrome_ws: Option<String>,
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
    #[arg(short = 'u', long, default_value = "BlackSparrow/1.0")]
    pub user_agent: String,
}
