//! # Streaming HTML Parser Module
//!
//! High-performance, zero-copy HTML parsing engine built on Cloudflare's [`lol_html`].
//!
//! Unlike traditional DOM-based parsers (e.g. `scraper`, `kuchiki`, `html5ever`) which build
//! an in-memory Abstract Syntax Tree (AST) representing every node in the document—consuming
//! 5x to 10x the raw HTML file size in heap allocations—SEO Lens utilizes a streaming,
//! event-driven tokenizer.
//!
//! ## Architectural Overview
//!
//! The parser evaluates pre-compiled CSS selectors directly over chunks of HTML as they are
//! received, extracting technical SEO metadata, heading structure, links, image resources,
//! JSON-LD schemas, and editorial content without ever storing the full DOM tree in memory:
//!
//! ```text
//! HTML Input Stream
//!        │
//!        ▼
//! ┌────────────────────────────────────────────────────────┐
//! │  lol_html Streaming Rewriter & CSS Selector Matcher    │
//! └──────┬──────────────┬──────────────┬──────────────┬────┘
//!        │              │              │              │
//!        ▼              ▼              ▼              ▼
//!   [Metadata]     [Headings]       [Links]      [Editorial Content]
//!   • Title        • H1 primary     • Anchor     • Word count
//!   • Description  • H1-H3 lists    • Target     • 64-bit ContentHash
//!   • Canonical    • H1 count       • Follow     • 64-bit SimHash
//!   • RobotsFlags                   • ImageLink
//!        │              │              │              │
//!        └──────────────┴──────┬───────┴──────────────┘
//!                              ▼
//!                        [ParsedPage]
//! ```
//!
//! ## Key Extracted Information
//!
//! 1. **Document Identity & Meta**: Page `<title>`, `<meta name="description">`, `<link rel="canonical">`,
//!    `lang` attribute, `charset`, `viewport`, and `<meta name="robots">` directives parsed into a 1-byte bitfield.
//! 2. **Heading Structure**: Primary `<h1>`, total `<h1>` count (for multi-H1 detection), and hierarchical
//!    lists of `<h2>` and `<h3>` tags.
//! 3. **Editorial Content & Duplicate Detection**:
//!    - Clean editorial body text excluding non-editorial elements (`<header>`, `<nav>`, `<footer>`, `<script>`, `<style>`).
//!    - Accurate alphanumeric word count.
//!    - 64-bit deterministic content hash for identifying exact duplicates.
//!    - 64-bit locality-sensitive SimHash fingerprint for detecting near-duplicate pages ($>85\%$ similarity).
//! 4. **Links & Navigation**: Fully resolved internal and external URLs, anchor texts, nofollow statuses, and image link flags.
//! 5. **Image Resources**: `src` URLs, `alt` text presence, and explicit `width`/`height` dimensions.
//! 6. **Structured Data**: Extraction and parsing of JSON-LD schemas (`<script type="application/ld+json">`) with
//!    support for `@graph` structures and Google Rich Results eligibility heuristics.
//! 7. **Social Metadata**: OpenGraph (`og:*`) and Twitter Card (`twitter:*`) tag pairs.
//!
//! ## Examples
//!
//! ```rust
//! use seo_lens::parser::parse_html;
//! use seo_lens::core::models::RobotsFlags;
//!
//! let html = r#"
//!     <!DOCTYPE html>
//!     <html lang="en">
//!     <head>
//!         <title>Product Launch | Example Corp</title>
//!         <meta name="description" content="Discover our next-generation platform.">
//!         <meta name="robots" content="noindex, nofollow">
//!         <meta property="og:title" content="Product Launch">
//!         <link rel="canonical" href="https://example.com/launch">
//!     </head>
//!     <body>
//!         <h1>Next-Gen Platform</h1>
//!         <p>Comprehensive technical audit engine and crawler in Rust.</p>
//!         <a href="/pricing">View Pricing</a>
//!     </body>
//!     </html>
//! "#;
//!
//! let parsed = parse_html(html, "https://example.com/launch").unwrap();
//!
//! assert_eq!(parsed.title.as_deref(), Some("Product Launch | Example Corp"));
//! assert_eq!(parsed.meta_description.as_deref(), Some("Discover our next-generation platform."));
//! assert_eq!(parsed.canonical_url.as_deref(), Some("https://example.com/launch"));
//! assert!(parsed.robots_flags.contains(RobotsFlags::NOINDEX));
//! assert!(parsed.robots_flags.contains(RobotsFlags::NOFOLLOW));
//! assert_eq!(parsed.h1_primary.as_deref(), Some("Next-Gen Platform"));
//! assert_eq!(parsed.get_open_graph("og:title").map(|s| s.as_str()), Some("Product Launch"));
//! assert_eq!(parsed.links.len(), 1);
//! assert_eq!(parsed.links[0].target_url, "https://example.com/pricing");
//! ```

pub mod content;
pub mod metadata;
pub mod schema;
pub mod streaming;

pub use streaming::parse_html;

use crate::core::models::{DiscoveredLink, HreflangTag, ImageResource, RobotsFlags, SchemaRecord};
use compact_str::CompactString;
use serde::{Deserialize, Serialize};

/// Extracted data structures resulting from parsing an HTML document.
///
/// Contains all metadata, heading elements, hyperlinks, image assets,
/// structured data, and content metrics required by SEO audit rules.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParsedPage {
    /// Document title extracted from `<title>...</title>`, with decoded HTML entities.
    pub title: Option<String>,
    /// Meta description content extracted from `<meta name="description" content="...">`.
    pub meta_description: Option<String>,
    /// Canonical URL extracted from `<link rel="canonical" href="...">`.
    pub canonical_url: Option<String>,
    /// Primary language code extracted from `<html lang="...">` (e.g. "en", "es-ES").
    pub html_lang: Option<CompactString>,
    /// Character set extracted from `<meta charset="...">` or `<meta http-equiv="Content-Type">`.
    pub charset: Option<CompactString>,
    /// Viewport configuration extracted from `<meta name="viewport" content="...">`.
    pub viewport: Option<CompactString>,
    /// Directives extracted from `<meta name="robots" content="...">` as a 1-byte bitfield.
    pub robots_flags: RobotsFlags,
    /// First `<h1>` heading encountered in the document.
    pub h1_primary: Option<String>,
    /// Total count of `<h1>` headings in the document (used to flag duplicate/multiple H1s).
    pub h1_count: u16,
    /// Ordered list of all `<h2>` heading texts found in the document.
    pub h2_headings: Vec<String>,
    /// Ordered list of all `<h3>` heading texts found in the document.
    pub h3_headings: Vec<String>,
    /// Total count of words in editorial text (excluding navigation, header, footer, script tags).
    pub word_count: u32,
    /// 64-bit deterministic hash of editorial body text for exact duplicate detection.
    pub content_hash: u64,
    /// 64-bit locality-sensitive SimHash fingerprint for near-duplicate detection.
    pub simhash: u64,
    /// All hyperlinks (`<a href="...">`) discovered on the page with resolved absolute URLs.
    pub links: Vec<DiscoveredLink>,
    /// All image assets (`<img src="...">`) with alt texts and dimension attributes.
    pub images: Vec<ImageResource>,
    /// All structured data records parsed from `<script type="application/ld+json">`.
    pub schemas: Vec<SchemaRecord>,
    /// International alternate language URLs extracted from `<link rel="alternate" hreflang="...">`.
    pub hreflangs: Vec<HreflangTag>,
    /// OpenGraph metadata key-value pairs (e.g. `("og:title", "...")`, `("og:image", "...")`).
    pub open_graph: Vec<(CompactString, String)>,
    /// Twitter Card metadata key-value pairs (e.g. `("twitter:card", "...")`).
    pub twitter_cards: Vec<(CompactString, String)>,
}

impl ParsedPage {
    /// Retrieves an OpenGraph property value if present (e.g. `"og:title"`, `"og:image"`).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use seo_lens::parser::ParsedPage;
    /// use compact_str::CompactString;
    ///
    /// let mut page = ParsedPage::default();
    /// page.open_graph.push((CompactString::new("og:title"), "My Article".to_string()));
    ///
    /// assert_eq!(page.get_open_graph("og:title").map(|s| s.as_str()), Some("My Article"));
    /// assert_eq!(page.get_open_graph("og:image"), None);
    /// ```
    pub fn get_open_graph(&self, property: &str) -> Option<&String> {
        self.open_graph
            .iter()
            .find(|(k, _)| k.as_str() == property)
            .map(|(_, v)| v)
    }

    /// Retrieves a Twitter Card property value if present (e.g. `"twitter:card"`, `"twitter:creator"`).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use seo_lens::parser::ParsedPage;
    /// use compact_str::CompactString;
    ///
    /// let mut page = ParsedPage::default();
    /// page.twitter_cards.push((CompactString::new("twitter:card"), "summary_large_image".to_string()));
    ///
    /// assert_eq!(page.get_twitter_card("twitter:card").map(|s| s.as_str()), Some("summary_large_image"));
    /// assert_eq!(page.get_twitter_card("twitter:creator"), None);
    /// ```
    pub fn get_twitter_card(&self, name: &str) -> Option<&String> {
        self.twitter_cards
            .iter()
            .find(|(k, _)| k.as_str() == name)
            .map(|(_, v)| v)
    }
}
