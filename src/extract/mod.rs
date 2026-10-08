//! # Agent-Mode Extraction
//!
//! Turns fetched pages into clean, token-cheap documents for AI agents and lets agents ask
//! for exactly what they need. No LLM is involved anywhere in this module.
//!
//! - [`types`]: the shared contract ([`PageDocument`], [`Block`], [`ScrapeOptions`], [`PageStatus`]).
//! - [`chunk`]: heading-based passages for search.
//! - [`tokens`]: o200k token counts and budget trimming.
//! - [`clean`]: junk, hidden-text and page-chrome removal.
//! - [`main_content`]: main-content detection (selectors, semantic tags, rs-trafilatura, density).
//! - [`convert`]: DOM to blocks, numbered link references and Markdown in one walk.
//! - [`document`]: HTML / Markdown / text to [`PageDocument`], formats and token budgets.
//! - [`scrape`]: the network side: guard, cache, robots.txt, Markdown fast path, Chrome.
//! - [`pdf`]: PDF text with page markers.
//! - [`map`]: fast URL discovery from sitemaps and page links.
//! - [`crawl`]: multi-page content crawl with cross-page [`boilerplate`] removal.
//! - [`jobs`]: background crawls with progress, pagination and cancel.
//! - [`sink`]: memory, NDJSON and Markdown-directory outputs.
//! - [`paths`]: include / exclude path patterns (globs and regexes).
//! - [`find`]: BM25 passage ranking, selector and regex matching.

pub mod boilerplate;
pub mod chunk;
pub mod clean;
pub mod convert;
pub mod crawl;
pub mod document;
pub mod find;
pub mod jobs;
pub mod main_content;
pub mod map;
pub mod markdown;
pub mod paths;
pub mod pdf;
pub mod scrape;
pub mod sink;
pub mod tokens;
pub mod types;

pub use document::{html_to_document, markdown_to_document, text_to_document};

pub use types::{
    unix_now, Block, BlockKind, BrowserAction, DocLink, DocMetadata, OutputFormat, PageDocument,
    PageStatus, RenderMode, ScrapeOptions, WaitUntil,
};
