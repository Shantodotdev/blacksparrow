//! # Black Sparrow
//!
//! High-performance, local-first website crawler, technical SEO audit engine,
//! and AI-native auditor written in Rust.
//!
//! ## Repository Architecture
//!
//! Black Sparrow is designed as an embeddable engine library (`blacksparrow`) and a headless CLI (`blacksparrow`).
//!
//! ### Core Engine Modules
//!
//! - [`core`]:
//!   - [`core::url`]: 8-stage URL normalization pipeline, RFC 3986 path resolution, tracking
//!     parameter stripping, and 64-bit SwissTable deduplication.
//!   - [`core::models`]: Core domain entities ([`core::models::PageReport`], [`core::models::RobotsFlags`],
//!     [`core::models::DiscoveredLink`], [`core::models::SchemaRecord`]).
//!   - [`core::config`]: Crawl execution parameters ([`core::config::CrawlConfig`]), politeness rates, and depth controls.
//! - [`crawler`]:
//!   - [`crawler::client`]: Asynchronous HTTP client wrapper with redirect tracking and TTFB timing.
//!   - [`crawler::aimd`]: Additive-Increase/Multiplicative-Decrease congestion controller.
//!   - [`crawler::waf`]: Anti-bot challenge fingerprint detection (Cloudflare, Akamai, DataDome).
//! - [`parser`]:
//!   - [`parser::streaming`]: Zero-copy streaming HTML parser powered by Cloudflare's `lol_html`.
//!   - [`parser::content`]: Word count extraction, 64-bit content hashing, and locality-sensitive 64-bit SimHash.
//!   - [`parser::metadata`]: RFC 9309 robots directive decoding, HTML entities, and canonicalization.
//! - [`graph`]:
//!   - [`graph::topology`]: Directed internal link topology graph ([`graph::SiteGraph`]) powered by `petgraph`.
//!   - [`graph::pagerank`]: Power-iteration internal PageRank link equity engine ([`graph::compute_pagerank`]).
//! - [`rules`]:
//!   - [`rules::catalog`]: Master catalog defining 120 technical SEO audit rules, severity tiers, and fix advice.
//!   - [`rules::page`]: Single-page in-flight rules engine ([`rules::evaluate_page`]).
//!   - [`rules::graph`]: Multi-page post-crawl graph rules engine ([`rules::evaluate_graph`]).
//! - [`error`]: Zero-panic error handling taxonomy ([`SeoError`], [`SeoResult`]).
//!
//! ## Quickstart Example
//!
//! ```rust
//! use blacksparrow::core::url::normalize_url;
//! use blacksparrow::parser::parse_html;
//! use blacksparrow::core::models::RobotsFlags;
//!
//! // 1. Normalize a messy incoming URL
//! let raw_url = "HTTPS://EXAMPLE.COM/blog//article?utm_source=twitter&b=2&a=1#section";
//! let clean_url = normalize_url(raw_url).unwrap();
//! assert_eq!(clean_url, "https://example.com/blog/article?a=1&b=2");
//!
//! // 2. Stream-parse an HTML document
//! let sample_html = r#"
//!     <!DOCTYPE html>
//!     <html>
//!     <head>
//!         <title>Understanding Rust Async Crawlers</title>
//!         <meta name="description" content="A guide to high-throughput crawlers.">
//!         <meta name="robots" content="noindex, nofollow">
//!         <link rel="canonical" href="https://example.com/blog/article">
//!     </head>
//!     <body>
//!         <h1>Rust Crawlers</h1>
//!         <p>Stream tokens without loading large DOM trees into memory.</p>
//!         <a href="/docs/api">API Reference</a>
//!     </body>
//!     </html>
//! "#;
//!
//! let page = parse_html(sample_html, &clean_url).unwrap();
//! assert_eq!(page.title.as_deref(), Some("Understanding Rust Async Crawlers"));
//! assert!(page.robots_flags.contains(RobotsFlags::NOINDEX));
//! assert_eq!(page.links[0].target_url, "https://example.com/docs/api");
//! ```

pub mod cli;
pub mod core;
pub mod crawler;
pub mod error;
pub mod graph;
pub mod mcp;
pub mod parser;
pub mod report;
pub mod rules;
pub mod storage;

pub use error::{SeoError, SeoResult};
