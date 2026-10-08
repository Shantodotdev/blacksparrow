//! Building [`PageDocument`]s from HTML, Markdown and plain text, and shaping them to the
//! formats a caller asked for.

use crate::core::models::RobotsFlags;
use crate::error::{SeoError, SeoResult};
use crate::extract::clean::{clean_document, CleanOptions};
use crate::extract::convert::{convert, render_markdown, render_text};
use crate::extract::main_content::{find_main_content, TrafilaturaDetector};
use crate::extract::markdown::markdown_to_blocks;
use crate::extract::tokens::{count_tokens, trim_blocks_to_budget};
use crate::extract::types::{
    BlockKind, DocLink, DocMetadata, OutputFormat, PageDocument, PageStatus, ScrapeOptions,
};
use crate::parser::content::compute_content_hash;
use crate::parser::{parse_html, ParsedPage};
use dom_query::{Document, NodeRef};
use url::Url;

/// Converts an HTML page into a clean document. Pure: no network, no browser.
///
/// # Errors
///
/// Returns [`SeoError::Url`] for an unparseable `url` and [`SeoError::Config`] for invalid
/// include or exclude selectors.
pub fn html_to_document(html: &str, url: &str, opts: &ScrapeOptions) -> SeoResult<PageDocument> {
    let base = Url::parse(url).map_err(|e| SeoError::Url(format!("Invalid URL '{url}': {e}")))?;
    let parsed = parse_html(html, url).unwrap_or_default();

    let dom = Document::from(html);
    clean_document(
        &dom,
        CleanOptions {
            only_main_content: opts.only_main_content,
            exclude_selectors: &opts.exclude_selectors,
        },
    )?;
    let main = find_main_content(
        &dom,
        url,
        &opts.include_selectors,
        opts.only_main_content,
        &TrafilaturaDetector,
    )?;
    let roots: Vec<NodeRef> = main
        .roots
        .iter()
        .map(|id| NodeRef::new(*id, &dom.tree))
        .collect();
    let converted = convert(&roots, &base);

    let mut doc = PageDocument::with_status(url, PageStatus::Ok);
    doc.source = "html".to_string();
    doc.content_type = "text/html".to_string();
    doc.extractor = main.method.to_string();
    doc.confidence = main.confidence;
    doc.metadata = metadata_from_parsed(&parsed);
    doc.metadata.author = main.meta.author.or(doc.metadata.author.take());
    doc.metadata.published = main.meta.published.or(doc.metadata.published.take());
    doc.metadata.site_name = main.meta.site_name.or(doc.metadata.site_name.take());
    doc.metadata.page_type = main.meta.page_type;
    doc.blocks = converted.blocks;
    doc.links = converted.links;
    doc.outlinks = outlinks(&parsed);
    if opts.wants(OutputFormat::Html) {
        doc.html = Some(roots.iter().map(|r| r.html().to_string()).collect());
    }
    if doc.metadata.title.is_none() {
        doc.metadata.title = first_heading(&doc);
    }
    finalize(&mut doc, opts);
    Ok(doc)
}

/// Builds a document from Markdown a server returned directly.
pub fn markdown_to_document(markdown: &str, url: &str, opts: &ScrapeOptions) -> PageDocument {
    let mut doc = PageDocument::with_status(url, PageStatus::Ok);
    doc.source = "markdown".to_string();
    doc.content_type = "text/markdown".to_string();
    doc.extractor = "server".to_string();
    doc.confidence = 1.0;
    doc.blocks = markdown_to_blocks(markdown);
    doc.metadata.title = first_heading(&doc);
    finalize(&mut doc, opts);
    // Keep the server's own Markdown (link style and all) unless it had to be trimmed.
    if !doc.truncated {
        doc.markdown = markdown.trim_end().to_string() + "\n";
        doc.tokens = count_tokens(&doc.markdown);
        doc.content_hash = compute_content_hash(&doc.markdown);
    }
    doc
}

/// Builds a document from plain text: each blank-line separated chunk is a paragraph.
pub fn text_to_document(text: &str, url: &str, opts: &ScrapeOptions) -> PageDocument {
    let mut doc = PageDocument::with_status(url, PageStatus::Ok);
    doc.source = "text".to_string();
    doc.content_type = "text/plain".to_string();
    doc.extractor = "text".to_string();
    doc.confidence = 1.0;
    doc.blocks = text
        .split("\n\n")
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .enumerate()
        .map(|(i, p)| crate::extract::types::Block {
            kind: BlockKind::Paragraph,
            text: p.to_string(),
            heading_path: Vec::new(),
            selector: format!("paragraph:{}", i + 1),
            markdown: p.to_string(),
        })
        .collect();
    finalize(&mut doc, opts);
    doc
}

/// Applies the token budget and fills markdown, text, tokens and content hash from blocks.
pub(crate) fn finalize(doc: &mut PageDocument, opts: &ScrapeOptions) {
    if let Some(max) = opts.max_tokens {
        let links_cost = |doc: &PageDocument| {
            count_tokens(&render_markdown(&doc.blocks, &doc.links))
                .saturating_sub(count_tokens(&render_markdown(&doc.blocks, &[])))
        };
        // Reserve room for the link reference list, then trim and re-check.
        let mut budget = max.saturating_sub(links_cost(doc));
        for _ in 0..4 {
            if trim_blocks_to_budget(&mut doc.blocks, budget) {
                doc.truncated = true;
            }
            let total = count_tokens(&render_markdown(&doc.blocks, &doc.links));
            if total <= max {
                break;
            }
            budget = budget.saturating_sub(total - max).max(1);
        }
    }
    doc.markdown = render_markdown(&doc.blocks, &doc.links);
    if doc.truncated {
        // Drop references no longer used by the trimmed body.
        doc.links.retain({
            let md = doc.markdown.clone();
            move |l| md.contains(&l.url)
        });
    }
    doc.text = render_text(&doc.blocks);
    doc.tokens = count_tokens(&doc.markdown);
    doc.content_hash = compute_content_hash(&doc.markdown);
}

/// All distinct link targets on the page with their anchor text.
fn outlinks(parsed: &ParsedPage) -> Vec<DocLink> {
    let mut seen = std::collections::HashSet::new();
    parsed
        .links
        .iter()
        .filter(|l| seen.insert(l.target_url.as_str()))
        .map(|l| DocLink {
            url: l.target_url.clone(),
            text: l
                .anchor_text
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" "),
        })
        .collect()
}

fn first_heading(doc: &PageDocument) -> Option<String> {
    doc.blocks
        .iter()
        .find(|b| matches!(b.kind, BlockKind::Heading { .. }))
        .map(|b| b.text.clone())
}

/// Page metadata from the streaming SEO parser.
pub fn metadata_from_parsed(parsed: &ParsedPage) -> DocMetadata {
    let mut meta = DocMetadata {
        title: parsed.title.clone().filter(|t| !t.trim().is_empty()),
        description: parsed.meta_description.clone(),
        language: parsed.html_lang.as_ref().map(|l| l.to_string()),
        canonical_url: parsed.canonical_url.clone(),
        ..Default::default()
    };
    for (key, value) in parsed.open_graph.iter().chain(parsed.twitter_cards.iter()) {
        meta.open_graph
            .entry(key.to_string())
            .or_insert_with(|| value.clone());
    }
    meta.site_name = meta.open_graph.get("og:site_name").cloned();
    meta.published = meta.open_graph.get("article:published_time").cloned();
    meta.author = meta.open_graph.get("article:author").cloned();
    meta.json_ld = parsed
        .schemas
        .iter()
        .filter_map(|s| serde_json::from_str(&s.raw_json).ok())
        .collect();
    for (flag, name) in [
        (RobotsFlags::NOINDEX, "noindex"),
        (RobotsFlags::NOFOLLOW, "nofollow"),
        (RobotsFlags::NOSNIPPET, "nosnippet"),
        (RobotsFlags::NOIMAGEINDEX, "noimageindex"),
        (RobotsFlags::NOARCHIVE, "noarchive"),
    ] {
        if parsed.robots_flags.contains(flag) {
            meta.robots.push(name.to_string());
        }
    }
    meta
}

impl PageDocument {
    /// Clears every field the caller did not ask for. Status, URLs, metadata needed to
    /// interpret the result (title, token count) and diagnostics are always kept.
    pub fn apply_formats(&mut self, opts: &ScrapeOptions) {
        if !opts.wants(OutputFormat::Markdown) {
            self.markdown.clear();
        }
        if !opts.wants(OutputFormat::Text) {
            self.text.clear();
        }
        if !opts.wants(OutputFormat::Json) {
            self.blocks.clear();
        }
        if !opts.wants(OutputFormat::Links) {
            self.links.clear();
            self.outlinks.clear();
        }
        if !opts.wants(OutputFormat::Html) {
            self.html = None;
        }
        if !opts.wants(OutputFormat::RawHtml) {
            self.raw_html = None;
        }
        if !opts.wants(OutputFormat::Screenshot) {
            self.screenshot = None;
        }
        if !opts.wants(OutputFormat::Metadata) {
            let title = self.metadata.title.take();
            self.metadata = DocMetadata {
                title,
                ..Default::default()
            };
        }
    }
}
