# SEO Lens: Data Models & Storage Schema Specification
**Document Status**: Permanent Technical Specification  
**Scope**: Rust Domain Models, Memory Layout, and SQLite WAL Database Schema

---

## 1. Architectural Philosophy & Memory Strategy

In a large crawl (10,000+ pages with 500,000+ internal links and assets), naive string allocations cause heap fragmentation, GC/allocator pauses, and gigabytes of wasted RAM. 

`SEO Lens` enforces strict memory principles:
1. **Stack-Inlined Strings (`compact_str::CompactString`)**:
   - Standard Rust `String` is 24 bytes pointing to a heap buffer.
   - `CompactString` stores up to 24 bytes directly on the stack (covering >80% of URL paths, titles, tags, and status codes) without touching the heap allocator.
2. **Compact Numeric Primitives**:
   - HTTP status codes are stored as `u16` (2 bytes).
   - Word counts, character lengths, and response times (TTFB in milliseconds) are stored as `u32` (4 bytes).
3. **Bitflags for Directives**:
   - Robots directives (`noindex`, `nofollow`, `noarchive`, `nosnippet`, `noimageindex`) are packed into a single 1-byte bitfield (`u8`) rather than 5 separate booleans or heap-allocated strings.
4. **Relational SQLite with Write-Ahead Logging (WAL)**:
   - Synchronous disk writes on every page destroy crawl throughput.
   - SQLite is configured in `WAL` mode with `PRAGMA synchronous = NORMAL`, writing in atomic batches of 250 pages.

---

## 2. Core Rust Domain Models

Below is the definitive specification of data structures in `src/core/models.rs`:

```rust
use compact_str::CompactString;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Severity classification for technical SEO findings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Critical = 1,
    Alert = 2,
    Warning = 3,
    Notice = 4,
}

/// Functional categories mapping to the 120-check catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IssueCategory {
    HttpTransport,
    TitleMetadata,
    Headings,
    Indexability,
    Canonicalization,
    Links,
    Security,
    MobileUx,
    Internationalization,
    StructuredData,
    GeoAiSearch,
    SiteGraph,
    JsDiff,
}

bitflags::bitflags! {
    /// Memory-efficient bitfield for robots and indexing directives.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
    pub struct RobotsFlags: u8 {
        const NONE         = 0b0000_0000;
        const NOINDEX      = 0b0000_0001;
        const NOFOLLOW     = 0b0000_0010;
        const NOSNIPPET    = 0b0000_0100;
        const NOIMAGEINDEX = 0b0000_1000;
        const NOARCHIVE    = 0b0001_0000;
    }
}

/// Represents the complete audit report for a single crawled URL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageReport {
    /// Unique incremental identifier (primary key in SQLite)
    pub id: Option<i64>,
    /// Associated crawl session identifier
    pub crawl_id: CompactString,
    
    // --- Network & Transport ---
    pub url: String,
    pub url_hash: u64,
    pub final_url: Option<String>,
    pub status_code: u16,
    pub content_type: CompactString,
    pub size_bytes: u32,
    pub ttfb_ms: u32,
    pub crawl_depth: u16,
    
    // --- Metadata ---
    pub title: Option<String>,
    pub title_length: u16,
    pub meta_description: Option<String>,
    pub meta_desc_length: u16,
    pub canonical_url: Option<String>,
    pub html_lang: Option<CompactString>,
    pub charset: Option<CompactString>,
    pub viewport: Option<CompactString>,
    
    // --- Directives ---
    pub robots_flags: RobotsFlags,
    pub is_sitemap_url: bool,
    pub is_internal: bool,
    
    // --- Headings ---
    pub h1_primary: Option<String>,
    pub h1_count: u16,
    pub h2_headings: Vec<String>,
    pub h3_headings: Vec<String>,
    
    // --- Content & Quality ---
    pub word_count: u32,
    pub content_hash: u64,
    pub simhash: u64,
    pub is_soft_404: bool,
    pub has_lorem_ipsum: bool,
    
    // --- Security ---
    pub is_https: bool,
    pub has_hsts: bool,
    pub has_csp: bool,
    pub has_x_frame: bool,
    pub has_x_content_type: bool,
    pub mixed_content_count: u16,
    
    // --- Child Collections (stored relationally) ---
    pub links: Vec<DiscoveredLink>,
    pub images: Vec<ImageResource>,
    pub schemas: Vec<SchemaRecord>,
    pub hreflangs: Vec<HreflangTag>,
    pub issues: Vec<IssueFinding>,
}

/// A hyperlink discovered in an HTML document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredLink {
    pub source_url: String,
    pub target_url: String,
    pub target_url_hash: u64,
    pub anchor_text: String,
    pub is_internal: bool,
    pub is_nofollow: bool,
    pub is_image_link: bool,
    pub status_code: Option<u16>,
}

/// An image asset referenced on a page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageResource {
    pub src_url: String,
    pub alt_text: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub size_bytes: Option<u32>,
    pub has_dimensions: bool,
    pub is_broken: bool,
}

/// JSON-LD or Microdata structured data block.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemaRecord {
    pub schema_type: CompactString,
    pub raw_json: String,
    pub is_valid_json: bool,
    pub is_google_eligible: bool,
    pub missing_required_fields: Vec<CompactString>,
}

/// Hreflang alternate language tag.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HreflangTag {
    pub lang_code: CompactString,
    pub target_url: String,
    pub is_reciprocal: bool,
}

/// A specific technical SEO defect identified by the rules engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IssueFinding {
    pub code: CompactString,       // e.g. "ERR_TITLE_MISSING"
    pub category: IssueCategory,
    pub severity: Severity,
    pub title: CompactString,      // Human-readable headline
    pub message: String,           // Context-specific detail
    pub target_url: String,
    pub source_page_url: Option<String>, // Which page linked here (for 404s/broken links)
}

/// Summary metrics for an entire crawl session.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CrawlSummary {
    pub session_id: String,
    pub target_url: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub total_pages_crawled: u32,
    pub total_links_discovered: u32,
    pub total_errors: u32,
    pub total_alerts: u32,
    pub total_warnings: u32,
    pub total_notices: u32,
    pub average_ttfb_ms: u32,
    pub p95_ttfb_ms: u32,
    pub health_score: u8,          // 0-100 score calculated by weighted severity
}
```

---

## 3. SQLite Relational Database Schema (DDL)

The schema is initialized via `src/storage/schema.sql`. It is tuned for write throughput during ingestion and fast indexed queries during reporting and MCP filtering.

```sql
-- Enables Write-Ahead Logging for high concurrent read/write throughput
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;
PRAGMA cache_size = -64000; -- 64MB page cache

-- Crawl Sessions table
CREATE TABLE IF NOT EXISTS crawls (
    session_id TEXT PRIMARY KEY,
    target_url TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('queued', 'crawling', 'analyzing_graph', 'completed', 'failed', 'paused')),
    total_max_pages INTEGER NOT NULL DEFAULT 10000,
    max_depth INTEGER NOT NULL DEFAULT 5,
    respect_robots BOOLEAN NOT NULL DEFAULT 1,
    render_js BOOLEAN NOT NULL DEFAULT 0,
    started_at TEXT NOT NULL,
    finished_at TEXT,
    total_pages INTEGER NOT NULL DEFAULT 0,
    error_count INTEGER NOT NULL DEFAULT 0,
    alert_count INTEGER NOT NULL DEFAULT 0,
    warning_count INTEGER NOT NULL DEFAULT 0,
    health_score INTEGER DEFAULT NULL
);

-- Crawled Pages table
CREATE TABLE IF NOT EXISTS pages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    crawl_id TEXT NOT NULL REFERENCES crawls(session_id) ON DELETE CASCADE,
    url TEXT NOT NULL,
    url_hash INTEGER NOT NULL,
    final_url TEXT,
    status_code INTEGER NOT NULL,
    content_type TEXT NOT NULL,
    size_bytes INTEGER NOT NULL DEFAULT 0,
    ttfb_ms INTEGER NOT NULL DEFAULT 0,
    crawl_depth INTEGER NOT NULL DEFAULT 0,
    title TEXT,
    title_length INTEGER NOT NULL DEFAULT 0,
    meta_description TEXT,
    meta_desc_length INTEGER NOT NULL DEFAULT 0,
    canonical_url TEXT,
    html_lang TEXT,
    charset TEXT,
    viewport TEXT,
    robots_flags INTEGER NOT NULL DEFAULT 0,
    is_sitemap_url BOOLEAN NOT NULL DEFAULT 0,
    is_internal BOOLEAN NOT NULL DEFAULT 1,
    h1_primary TEXT,
    h1_count INTEGER NOT NULL DEFAULT 0,
    word_count INTEGER NOT NULL DEFAULT 0,
    content_hash INTEGER NOT NULL DEFAULT 0,
    simhash INTEGER NOT NULL DEFAULT 0,
    is_https BOOLEAN NOT NULL DEFAULT 1,
    has_hsts BOOLEAN NOT NULL DEFAULT 0,
    has_csp BOOLEAN NOT NULL DEFAULT 0,
    has_x_frame BOOLEAN NOT NULL DEFAULT 0,
    has_x_content_type BOOLEAN NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Links Graph table (Internal and External)
CREATE TABLE IF NOT EXISTS links (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    crawl_id TEXT NOT NULL REFERENCES crawls(session_id) ON DELETE CASCADE,
    source_url TEXT NOT NULL,
    target_url TEXT NOT NULL,
    target_url_hash INTEGER NOT NULL,
    anchor_text TEXT NOT NULL DEFAULT '',
    is_internal BOOLEAN NOT NULL DEFAULT 1,
    is_nofollow BOOLEAN NOT NULL DEFAULT 0,
    status_code INTEGER DEFAULT NULL
);

-- Technical Issues table
CREATE TABLE IF NOT EXISTS issues (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    crawl_id TEXT NOT NULL REFERENCES crawls(session_id) ON DELETE CASCADE,
    page_id INTEGER REFERENCES pages(id) ON DELETE CASCADE,
    code TEXT NOT NULL,
    category TEXT NOT NULL,
    severity INTEGER NOT NULL CHECK(severity IN (1, 2, 3, 4)),
    title TEXT NOT NULL,
    message TEXT NOT NULL,
    target_url TEXT NOT NULL,
    source_page_url TEXT
);

-- Structured Data (JSON-LD) table
CREATE TABLE IF NOT EXISTS schemas (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    crawl_id TEXT NOT NULL REFERENCES crawls(session_id) ON DELETE CASCADE,
    page_id INTEGER NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    schema_type TEXT NOT NULL,
    raw_json TEXT NOT NULL,
    is_valid_json BOOLEAN NOT NULL DEFAULT 1,
    is_google_eligible BOOLEAN NOT NULL DEFAULT 1,
    missing_required TEXT NOT NULL DEFAULT '[]'
);

-- Images table
CREATE TABLE IF NOT EXISTS images (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    crawl_id TEXT NOT NULL REFERENCES crawls(session_id) ON DELETE CASCADE,
    page_id INTEGER NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    src_url TEXT NOT NULL,
    alt_text TEXT,
    width INTEGER,
    height INTEGER,
    has_dimensions BOOLEAN NOT NULL DEFAULT 0,
    is_broken BOOLEAN NOT NULL DEFAULT 0
);

-- Indexes for Microsecond Querying
CREATE INDEX IF NOT EXISTS idx_pages_crawl_hash ON pages(crawl_id, url_hash);
CREATE INDEX IF NOT EXISTS idx_pages_status ON pages(crawl_id, status_code);
CREATE INDEX IF NOT EXISTS idx_links_crawl_target ON links(crawl_id, target_url_hash);
CREATE INDEX IF NOT EXISTS idx_links_source ON links(crawl_id, source_url);
CREATE INDEX IF NOT EXISTS idx_issues_crawl_severity ON issues(crawl_id, severity);
CREATE INDEX IF NOT EXISTS idx_issues_crawl_category ON issues(crawl_id, category);
CREATE INDEX IF NOT EXISTS idx_issues_code ON issues(crawl_id, code);
```

---

## 4. Batch Transaction Ingestion Contract

To ensure the database layer never blocks the crawler:
1. **Batch Accumulator**:
   - As worker green tasks complete, `PageReport` structs are sent over a `tokio::sync::mpsc::channel(1000)` to a dedicated SQLite writer task.
   - The writer accumulates up to **250 pages** or flushes every **2.0 seconds** (whichever occurs first).
2. **Transaction Batching**:
   - `BEGIN TRANSACTION` $\rightarrow$ bulk insert pages, bulk insert links, bulk insert issues $\rightarrow$ `COMMIT`.
   - Bypasses disk flush thrashing, sustaining thousands of page writes per second on standard SSDs.

---

## 5. Summary

This specification guarantees:
- Zero data model ambiguity between the crawler, parser, rules engine, and SQLite storage.
- Microscopic memory overhead using stack-allocated `CompactString` and packed `RobotsFlags`.
- Deterministic, high-throughput relational persistence for all 120 SEO checks.
