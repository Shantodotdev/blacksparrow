//! Shared contract types for agent-mode extraction: options in, page documents out.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Outcome classification for a scraped page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PageStatus {
    /// Content was extracted.
    #[default]
    Ok,
    /// A bot challenge or WAF block page was served instead of content.
    Blocked,
    /// The response was neither HTML, Markdown, plain text nor PDF.
    NotHtml,
    /// The response exceeded the streaming size cap.
    TooLarge,
    /// The fetch or extraction failed.
    Error,
    /// A PDF with no extractable text (scanned images); OCR would be needed.
    NeedsOcr,
}

impl PageStatus {
    /// Stable lowercase name used in storage and APIs.
    pub fn as_str(&self) -> &'static str {
        match self {
            PageStatus::Ok => "ok",
            PageStatus::Blocked => "blocked",
            PageStatus::NotHtml => "not_html",
            PageStatus::TooLarge => "too_large",
            PageStatus::Error => "error",
            PageStatus::NeedsOcr => "needs_ocr",
        }
    }
}

/// When to use Chrome for a page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RenderMode {
    /// Render only when the raw HTML looks like an empty single-page-app shell.
    #[default]
    Auto,
    /// Never start Chrome.
    Never,
    /// Always render in Chrome (skips the raw HTTP download).
    Always,
}

/// Output formats a caller can ask for. Fields not requested are left empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputFormat {
    /// Clean GitHub-flavoured Markdown.
    Markdown,
    /// Typed content blocks with heading paths.
    Json,
    /// Plain text of the main content.
    Text,
    /// Links found in the main content.
    Links,
    /// Title, description, OpenGraph, JSON-LD and other metadata.
    Metadata,
    /// Cleaned main-content HTML.
    Html,
    /// The raw response body.
    RawHtml,
    /// A PNG screenshot (rendered mode only).
    Screenshot,
}

/// A browser step run before the page is read (rendered mode only).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BrowserAction {
    /// Click an element by CSS selector or snapshot reference (`e12`).
    Click {
        /// CSS selector or accessibility snapshot reference.
        target: String,
    },
    /// Type text into an element.
    Type {
        /// CSS selector or accessibility snapshot reference.
        target: String,
        /// Text to type.
        text: String,
    },
    /// Press a keyboard key (e.g. `Enter`).
    Press {
        /// Key name.
        key: String,
    },
    /// Scroll to the bottom of the page `times` times.
    Scroll {
        /// Number of scrolls (default 1).
        #[serde(default = "one")]
        times: u32,
    },
    /// Wait until a selector appears.
    WaitFor {
        /// CSS selector.
        selector: String,
    },
    /// Wait a fixed number of milliseconds.
    Wait {
        /// Milliseconds.
        ms: u64,
    },
}

fn one() -> u32 {
    1
}

/// When the renderer considers a page ready to read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WaitUntil {
    /// The `load` event fired.
    Load,
    /// `DOMContentLoaded` fired (navigation always waits for `load`, which comes after it).
    DomReady,
    /// `load` fired and no new network requests for 500 ms.
    #[default]
    NetworkIdle,
}

/// Options for scraping one page. Field names also accept the Firecrawl spelling.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScrapeOptions {
    /// Requested output formats.
    pub formats: Vec<OutputFormat>,
    /// Keep only the main content (drop navigation, header, footer, sidebars).
    #[serde(alias = "onlyMainContent")]
    pub only_main_content: bool,
    /// CSS selectors whose elements form the content (overrides main-content detection).
    #[serde(alias = "includeTags", alias = "include")]
    pub include_selectors: Vec<String>,
    /// CSS selectors removed before extraction.
    #[serde(alias = "excludeTags", alias = "exclude")]
    pub exclude_selectors: Vec<String>,
    /// Trim the Markdown to roughly this many tokens, keeping every heading.
    #[serde(alias = "maxTokens")]
    pub max_tokens: Option<usize>,
    /// Chrome rendering policy.
    pub render: RenderMode,
    /// CSS selector to wait for before reading (implies rendering).
    #[serde(alias = "waitForSelector")]
    pub wait_for_selector: Option<String>,
    /// Fixed wait in milliseconds after load (Firecrawl `waitFor`).
    #[serde(alias = "waitFor")]
    pub wait_ms: Option<u64>,
    /// Readiness condition for rendered pages.
    #[serde(alias = "waitUntil")]
    pub wait_until: WaitUntil,
    /// Steps to run in the browser before reading (implies rendering).
    pub actions: Vec<BrowserAction>,
    /// Ask servers for Markdown first (`Accept: text/markdown`).
    #[serde(alias = "acceptMarkdown")]
    pub accept_markdown: bool,
    /// Return a stored copy younger than this many seconds instead of fetching.
    #[serde(alias = "maxAge")]
    pub max_age_secs: Option<u64>,
    /// Per-page timeout in milliseconds.
    pub timeout_ms: Option<u64>,
}

impl Default for ScrapeOptions {
    fn default() -> Self {
        Self {
            formats: vec![OutputFormat::Markdown, OutputFormat::Metadata],
            only_main_content: true,
            include_selectors: Vec::new(),
            exclude_selectors: Vec::new(),
            max_tokens: None,
            render: RenderMode::Auto,
            wait_for_selector: None,
            wait_ms: None,
            wait_until: WaitUntil::NetworkIdle,
            actions: Vec::new(),
            accept_markdown: true,
            max_age_secs: None,
            timeout_ms: None,
        }
    }
}

impl ScrapeOptions {
    /// Whether a format was requested.
    pub fn wants(&self, format: OutputFormat) -> bool {
        self.formats.contains(&format)
    }

    /// Whether these options require Chrome regardless of the page.
    pub fn requires_browser(&self) -> bool {
        self.render == RenderMode::Always
            || self.wait_for_selector.is_some()
            || !self.actions.is_empty()
            || self.wants(OutputFormat::Screenshot)
    }
}

/// The kind of a content block, with kind-specific data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BlockKind {
    /// A heading (`h1`–`h6`).
    Heading {
        /// Heading level 1–6.
        level: u8,
    },
    /// A paragraph of prose.
    Paragraph,
    /// A bulleted or numbered list.
    List {
        /// Whether the list is numbered.
        ordered: bool,
        /// Plain-text list items.
        items: Vec<String>,
    },
    /// A table.
    Table {
        /// Header cells (empty when the table has no header row).
        header: Vec<String>,
        /// Body rows.
        rows: Vec<Vec<String>>,
    },
    /// A preformatted code block.
    Code {
        /// Language hint from `language-*` / `lang-*` classes.
        language: Option<String>,
    },
    /// A blockquote.
    Quote,
    /// A standalone image.
    Image {
        /// Absolute image URL.
        src: String,
        /// Alternative text.
        alt: String,
    },
}

/// One structural block of a page document.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Block {
    /// Block kind and kind-specific data.
    #[serde(flatten)]
    pub kind: BlockKind,
    /// Plain text of the block.
    pub text: String,
    /// Headings this block sits under, outermost first.
    pub heading_path: Vec<String>,
    /// A CSS selector for the source element.
    pub selector: String,
    /// Markdown rendering of the block.
    #[serde(skip)]
    pub markdown: String,
}

/// A link found in the page content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocLink {
    /// Absolute URL.
    pub url: String,
    /// Anchor text.
    pub text: String,
}

/// Page-level metadata.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DocMetadata {
    /// Document title.
    pub title: Option<String>,
    /// Meta description.
    pub description: Option<String>,
    /// `<html lang>`.
    pub language: Option<String>,
    /// Canonical URL.
    pub canonical_url: Option<String>,
    /// Author when detectable.
    pub author: Option<String>,
    /// Publication date (RFC 3339) when detectable.
    pub published: Option<String>,
    /// Site name.
    pub site_name: Option<String>,
    /// OpenGraph and Twitter card properties.
    pub open_graph: BTreeMap<String, String>,
    /// Parsed JSON-LD blocks.
    pub json_ld: Vec<Value>,
    /// The site's `Content-Signal` header, if any (e.g. `ai-input=yes`).
    pub content_signal: Option<String>,
    /// Robots meta directives, lowercase.
    pub robots: Vec<String>,
    /// Page type label from main-content detection (article, product, listing, ...).
    pub page_type: Option<String>,
}

/// One clean page, the unit every agent verb works with.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PageDocument {
    /// Requested URL.
    pub url: String,
    /// URL after redirects and client-side navigation.
    pub final_url: String,
    /// Outcome classification.
    pub status: PageStatus,
    /// HTTP status code (0 when no response).
    pub status_code: u16,
    /// WAF vendor when `status` is `blocked`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked_by: Option<String>,
    /// Error message when `status` is `error`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Response content type.
    pub content_type: String,
    /// Where the content came from: `html`, `rendered`, `markdown`, `text` or `pdf`.
    pub source: String,
    /// Page metadata.
    pub metadata: DocMetadata,
    /// Clean Markdown.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub markdown: String,
    /// Plain text.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub text: String,
    /// Content blocks.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<Block>,
    /// Links in the content.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<DocLink>,
    /// Every link on the page (navigation included), deduplicated. Used for crawling and
    /// returned with the `links` format.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub outlinks: Vec<DocLink>,
    /// Cleaned main-content HTML.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
    /// Raw response body.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_html: Option<String>,
    /// Base64 PNG screenshot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub screenshot: Option<String>,
    /// Token count of `markdown` (o200k_base).
    pub tokens: usize,
    /// Whether `max_tokens` trimmed the content.
    pub truncated: bool,
    /// Main-content method used (`selectors`, `semantic`, `trafilatura`, `density`, `full`).
    pub extractor: String,
    /// Confidence 0–1 that the main content was found.
    pub confidence: f64,
    /// 64-bit hash of the Markdown, for change tracking.
    pub content_hash: u64,
    /// Unix seconds when the page was fetched.
    pub fetched_at: u64,
    /// Whether this copy came from the cache.
    pub from_cache: bool,
    /// Whether the content changed since the previous stored copy (`None` = first seen).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub changed: Option<bool>,
}

impl PageDocument {
    /// Creates an empty document for `url` with the given status.
    pub fn with_status(url: &str, status: PageStatus) -> Self {
        Self {
            url: url.to_string(),
            final_url: url.to_string(),
            status,
            fetched_at: unix_now(),
            ..Default::default()
        }
    }

    /// Host of the final URL, lowercase.
    pub fn host(&self) -> String {
        url::Url::parse(&self.final_url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
            .unwrap_or_default()
    }
}

/// Current Unix time in seconds.
pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
