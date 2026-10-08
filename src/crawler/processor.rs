//! # Page Processor Seam
//!
//! Splits per-page crawl work into two halves:
//!
//! 1. **Fetch** (shared): HTTP fetch, optional Chrome render, and HTML parse into a
//!    [`FetchedPage`]. Frontier, robots, AIMD politeness and the private-network guard all
//!    live on this side and are identical for every crawl mode.
//! 2. **Process** (pluggable): a [`PageProcessor`] turns the fetched page into an output.
//!    [`SeoProcessor`] produces today's [`PageReport`] with the 120 audit rules;
//!    [`crate::extract::ContentProcessor`] produces clean agent documents.

use crate::core::models::{IssueFinding, PageReport};
use crate::core::url::{contains_ignore_ascii_case, url_hash};
use crate::crawler::client::FetchResult;
use crate::crawler::frontier::FrontierEntry;
use crate::crawler::render::RenderedDocument;
use crate::parser::ParsedPage;
use crate::rules::{evaluate_js_diff, evaluate_page};
use compact_str::CompactString;

/// A page after the shared fetch step, ready for a [`PageProcessor`].
#[derive(Debug)]
pub struct FetchedPage {
    /// Frontier entry that produced this fetch.
    pub entry: FrontierEntry,
    /// Raw HTTP response.
    pub fetch: FetchResult,
    /// Chrome rendering outcome when rendering ran for this page.
    pub rendered: Option<Result<RenderedDocument, String>>,
    /// Primary parse: the rendered DOM when rendering succeeded, otherwise the raw HTML.
    /// `None` for non-HTML responses.
    pub parsed: Option<ParsedPage>,
    /// Raw HTML parse, kept only when rendering ran so processors can diff the two.
    pub raw_parsed: Option<ParsedPage>,
}

/// Turns a [`FetchedPage`] into a mode-specific output.
pub trait PageProcessor: Send + Sync + 'static {
    /// The output produced for each page.
    type Output: Send + 'static;

    /// Processes one fetched page. Must not panic; failures are encoded in the output.
    fn process(&self, page: FetchedPage) -> Self::Output;
}

/// The technical SEO audit processor: evaluates single-page and JS-diff rules and builds a
/// [`PageReport`].
#[derive(Debug, Clone)]
pub struct SeoProcessor {
    /// Crawl session identifier stamped onto every report.
    pub session_id: String,
}

impl PageProcessor for SeoProcessor {
    type Output = PageReport;

    fn process(&self, page: FetchedPage) -> PageReport {
        let FetchedPage {
            entry,
            fetch,
            rendered,
            parsed,
            raw_parsed,
        } = page;

        let js_issues = match (raw_parsed.as_ref(), rendered.as_ref(), parsed.as_ref()) {
            (Some(raw), Some(Ok(document)), Some(rendered_page)) => evaluate_js_diff(
                raw,
                rendered_page,
                &fetch.final_url,
                &document.final_url,
                &document.runtime_errors,
            ),
            (Some(raw), Some(Err(message)), _) => evaluate_js_diff(
                raw,
                &ParsedPage::default(),
                &fetch.final_url,
                &fetch.final_url,
                std::slice::from_ref(message),
            ),
            _ => Vec::new(),
        };

        let mut page_issues = match parsed.as_ref() {
            Some(p) => evaluate_page(p, &fetch),
            None => Vec::new(),
        };
        page_issues.extend(js_issues);

        build_page_report(
            &self.session_id,
            entry.url.as_str(),
            entry.depth,
            &fetch,
            parsed,
            page_issues,
        )
    }
}

/// Builds the persisted SEO page report, moving parsed structures without cloning.
pub(crate) fn build_page_report(
    session_id: &str,
    url: &str,
    depth: u16,
    res: &FetchResult,
    parsed: Option<ParsedPage>,
    issues: Vec<IssueFinding>,
) -> PageReport {
    let mut report = PageReport {
        crawl_id: CompactString::new(session_id),
        url: url.to_string(),
        url_hash: url_hash(url),
        final_url: Some(res.final_url.clone()),
        status_code: res.status_code,
        content_type: CompactString::new(&res.content_type),
        size_bytes: res.size_bytes,
        ttfb_ms: res.ttfb_ms,
        crawl_depth: depth,
        is_internal: true,
        // Allocation-free case-insensitive substring search over raw body bytes
        // avoids allocating full lowercased copies of HTML documents (up to 2.5GB across 50k pages).
        has_lorem_ipsum: contains_ignore_ascii_case(&res.body, "lorem ipsum"),
        is_https: url.starts_with("https://"),
        has_hsts: res.headers.contains_key("strict-transport-security"),
        has_csp: res.headers.contains_key("content-security-policy"),
        has_x_frame: res.headers.contains_key("x-frame-options"),
        has_x_content_type: res.headers.contains_key("x-content-type-options"),
        issues,
        ..Default::default()
    };

    // Zero-copy move semantics: transfer ownership of parsed structures (links, headings,
    // images, JSON-LD schemas, hreflangs) directly into the page report without cloning.
    if let Some(p) = parsed {
        report.title = p.title;
        report.title_length = report.title.as_ref().map(|t| t.len() as u16).unwrap_or(0);
        report.meta_description = p.meta_description;
        report.meta_desc_length = report
            .meta_description
            .as_ref()
            .map(|d| d.len() as u16)
            .unwrap_or(0);
        report.canonical_url = p.canonical_url;
        report.html_lang = p.html_lang;
        report.charset = p.charset;
        report.viewport = p.viewport;
        report.robots_flags = p.robots_flags;
        report.h1_primary = p.h1_primary;
        report.h1_count = p.h1_count;
        report.h2_headings = p.h2_headings;
        report.h3_headings = p.h3_headings;
        report.word_count = p.word_count;
        report.content_hash = p.content_hash;
        report.simhash = p.simhash;
        report.links = p.links;
        report.images = p.images;
        report.schemas = p.schemas;
        report.hreflangs = p.hreflangs;
        report.page_intent = p.page_intent;
    }

    report
}
