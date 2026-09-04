//! # Domain Models
//!
//! Core data models representing pages, discovered links, images, schema records,
//! SEO issues, severity tiers, and crawl summary statistics.

use compact_str::CompactString;
use serde::{Deserialize, Serialize};

/// Severity classification for technical SEO findings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Critical = 1,
    Alert = 2,
    Warning = 3,
    Notice = 4,
}

/// Functional categories mapping to the 120-check technical SEO catalog.
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
    /// Unique issue identifier code (e.g. "ERR_TITLE_MISSING")
    pub code: CompactString,
    pub category: IssueCategory,
    pub severity: Severity,
    /// Human-readable headline
    pub title: CompactString,
    /// Context-specific detail explaining where and why it failed
    pub message: String,
    pub target_url: String,
    /// Source page that linked to this target (useful for 404s/broken links)
    pub source_page_url: Option<String>,
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
    /// 0-100 score calculated by weighted severity
    pub health_score: u8,
}
