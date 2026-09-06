//! # Crawler Module
//!
//! Asynchronous network fetching engine, AIMD adaptive politeness controller,
//! and WAF / bot challenge fingerprint probes.
//!
//! ## Modules
//!
//! - [`client`]: Asynchronous HTTP client wrapper around `reqwest` with manual redirect tracking and TTFB timing.
//! - [`aimd`]: Additive-Increase/Multiplicative-Decrease congestion controller protecting origin servers.
//! - [`waf`]: Bot challenge fingerprint scanner detecting Cloudflare, Akamai, DataDome, and Imperva screens.
//! - [`frontier`]: SwissTable hash deduplication and BFS/DFS frontier queue scheduler.
//! - [`robots`]: RFC 9309 compliant `robots.txt` rule evaluator and crawl-delay parser.
//! - [`sitemap`]: Streaming XML sitemap parser with alternates and gzip decompression.

pub mod aimd;
pub mod client;
pub mod engine;
pub mod frontier;
pub mod inspector;
pub mod priority;
pub mod robots;
pub mod sitemap;
pub mod waf;

pub use aimd::AimdController;
pub use client::{FetchOptions, FetchResult, HttpClient};
pub use engine::{
    run_crawl, run_crawl_with_options, CrawlResult, ProgressCallback, ProgressUpdate,
};
pub use frontier::{Frontier, FrontierEntry};
pub use inspector::inspect_url;
pub use priority::{calculate_url_importance, is_pagination_url, parse_url_segments};
pub use robots::RobotsTxt;
pub use sitemap::{parse_sitemap, SitemapDocument, SitemapEntry, SitemapIndexEntry};
pub use waf::detect_waf;
