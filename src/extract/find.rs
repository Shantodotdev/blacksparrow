//! `find`: return only the parts of a page (or a crawl) that answer a request.
//!
//! - Query mode ranks heading-based passages with BM25, boosting passages whose headings hold
//!   query words and expanding common field words with [`synonyms`](crate::extract::synonyms).
//!   Across a crawl it uses the SQLite FTS5 index filled while crawling.
//! - Selector mode returns each CSS match's text, chosen attributes or outer HTML.
//! - Regex mode returns each match with surrounding context.
//!
//! Every hit carries its URL, heading path and a CSS selector for the element it came from,
//! so an agent can reuse the selector with selector mode or `extract`. Matching always runs on
//! cleaned content, so hidden text is never returned.

use crate::error::{SeoError, SeoResult};
use crate::extract::chunk::chunk_blocks;
use crate::extract::clean::{clean_document, node_name, try_select, CleanOptions};
use crate::extract::convert::css_path;
use crate::extract::fields::records::visible_text;
use crate::extract::scrape::Scraper;
use crate::extract::synonyms::expand_term;
use crate::extract::types::{OutputFormat, PageDocument, PageStatus, ScrapeOptions};
use crate::storage::documents::{documents_for_crawl, search_chunks, ChunkQuery};
use dom_query::{Document, NodeId, NodeRef};
use hashbrown::{HashMap, HashSet};
use regex::Regex;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Common English words ignored when building search queries.
const STOPWORDS: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "by", "can", "do", "does", "for", "from", "how",
    "i", "if", "in", "is", "it", "its", "me", "my", "of", "on", "or", "our", "so", "than", "that",
    "the", "their", "there", "this", "to", "was", "we", "what", "when", "where", "which", "who",
    "why", "will", "with", "you", "your",
];

/// BM25 term-frequency saturation.
const BM25_K1: f64 = 1.2;
/// BM25 length normalisation.
const BM25_B: f64 = 0.75;
/// Weight of a synonym relative to the word the caller typed.
const SYNONYM_WEIGHT: f64 = 0.5;
/// Extra weight when a query word appears in the passage's heading path.
const HEADING_BOOST: f64 = 0.6;

/// Lowercase alphanumeric terms of a query. With `drop_stopwords`, common words are removed
/// (unless that would leave nothing).
pub fn query_terms(query: &str, drop_stopwords: bool) -> Vec<String> {
    let all: Vec<String> = tokenize(query);
    if !drop_stopwords {
        return all;
    }
    let kept: Vec<String> = all
        .iter()
        .filter(|t| !STOPWORDS.contains(&t.as_str()))
        .cloned()
        .collect();
    if kept.is_empty() {
        all
    } else {
        kept
    }
}

/// Splits text into lowercase alphanumeric tokens.
pub fn tokenize(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Folds simple English inflections so `retries`, `retried` and `retry` match.
fn stem(token: &str) -> String {
    let n = token.chars().count();
    if n > 4 && (token.ends_with("ies") || token.ends_with("ied")) {
        return format!("{}y", &token[..token.len() - 3]);
    }
    if n > 3 && token.ends_with('s') && !token.ends_with("ss") && !token.ends_with("us") {
        return token[..token.len() - 1].to_string();
    }
    token.to_string()
}

/// Query terms with synonyms, each with its weight.
pub(crate) fn weighted_terms(query: &str) -> Vec<(String, f64)> {
    let mut terms: Vec<(String, f64)> = Vec::new();
    let mut push = |term: String, weight: f64| {
        if let Some(existing) = terms.iter_mut().find(|(t, _)| *t == term) {
            existing.1 = existing.1.max(weight);
        } else {
            terms.push((term, weight));
        }
    };
    for term in query_terms(query, true) {
        for synonym in expand_term(&term) {
            push(stem(synonym), SYNONYM_WEIGHT);
        }
        push(stem(&term), 1.0);
    }
    terms
}

/// Options shared by all find modes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FindOptions {
    /// Passages returned in query mode.
    #[serde(alias = "topK")]
    pub top_k: usize,
    /// Target passage size in tokens.
    pub chunk_tokens: usize,
    /// Attributes returned for each selector match (`href` and `src` are made absolute).
    pub attributes: Vec<String>,
    /// Include each selector match's outer HTML.
    #[serde(alias = "outerHtml")]
    pub outer_html: bool,
    /// Maximum selector or regex matches per page.
    #[serde(alias = "maxMatches")]
    pub max_matches: usize,
    /// Characters of context on each side of a regex match.
    pub context_chars: usize,
}

impl Default for FindOptions {
    fn default() -> Self {
        Self {
            top_k: 5,
            chunk_tokens: 200,
            attributes: Vec::new(),
            outer_html: false,
            max_matches: 50,
            context_chars: 80,
        }
    }
}

/// A find request: a page (`url`) or stored crawl results (`crawl_id`, `host`, `url_prefix`),
/// and exactly one of `query`, `selector` or `regex`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FindRequest {
    /// Page to fetch and search.
    pub url: Option<String>,
    /// Search the documents of this crawl.
    #[serde(alias = "crawlId")]
    pub crawl_id: Option<String>,
    /// Restrict stored results to one host.
    pub host: Option<String>,
    /// Restrict stored results to URLs starting with this prefix.
    #[serde(alias = "urlPrefix")]
    pub url_prefix: Option<String>,
    /// Keywords or a question.
    pub query: Option<String>,
    /// CSS selector.
    pub selector: Option<String>,
    /// Regular expression.
    pub regex: Option<String>,
    /// Mode options.
    #[serde(flatten)]
    pub options: FindOptions,
    /// How the page is scraped (url mode).
    #[serde(alias = "scrapeOptions")]
    pub scrape: ScrapeOptions,
}

/// One match.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FindHit {
    /// Page URL.
    pub url: String,
    /// Passage, element text or matched string.
    pub text: String,
    /// Headings above the match, outermost first.
    pub heading_path: Vec<String>,
    /// CSS selector of the element the match came from.
    pub selector: String,
    /// Relevance (query mode).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    /// Highlighted passage (crawl queries) or surrounding context (regex mode).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
    /// Requested attributes (selector mode).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, String>,
    /// Outer HTML (selector mode, when requested).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
}

/// Result of a find request.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FindResult {
    /// `query`, `selector` or `regex`.
    pub mode: String,
    /// Page searched (url mode).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Page status (url mode); hits are empty unless `ok`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<PageStatus>,
    /// Why the page could not be searched.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Matches, best first in query mode, page order otherwise.
    pub hits: Vec<FindHit>,
}

enum Mode<'a> {
    Query(&'a str),
    Selector(&'a str),
    Regex(&'a str),
}

impl FindRequest {
    fn mode(&self) -> SeoResult<Mode<'_>> {
        let set: Vec<Mode<'_>> = [
            self.query
                .as_deref()
                .filter(|q| !q.trim().is_empty())
                .map(Mode::Query),
            self.selector
                .as_deref()
                .filter(|q| !q.trim().is_empty())
                .map(Mode::Selector),
            self.regex
                .as_deref()
                .filter(|q| !q.is_empty())
                .map(Mode::Regex),
        ]
        .into_iter()
        .flatten()
        .collect();
        match set.len() {
            1 => set
                .into_iter()
                .next()
                .ok_or_else(|| SeoError::Config("Nothing to find".to_string())),
            0 => Err(SeoError::Config(
                "Provide one of query, selector or regex".to_string(),
            )),
            _ => Err(SeoError::Config(
                "Provide only one of query, selector or regex".to_string(),
            )),
        }
    }
}

/// Ranks a document's passages against `query` with BM25 and returns the best `top_k`.
pub fn find_in_document(doc: &PageDocument, query: &str, top_k: usize) -> Vec<FindHit> {
    find_in_document_with(doc, query, top_k, FindOptions::default().chunk_tokens)
}

fn find_in_document_with(
    doc: &PageDocument,
    query: &str,
    top_k: usize,
    chunk_tokens: usize,
) -> Vec<FindHit> {
    let terms = weighted_terms(query);
    let chunks = chunk_blocks(&doc.blocks, chunk_tokens.max(20));
    if terms.is_empty() || chunks.is_empty() {
        return Vec::new();
    }
    let bodies: Vec<Vec<String>> = chunks
        .iter()
        .map(|c| tokenize(&c.text).iter().map(|t| stem(t)).collect())
        .collect();
    let headings: Vec<HashSet<String>> = chunks
        .iter()
        .map(|c| {
            tokenize(&c.heading_path.join(" "))
                .iter()
                .map(|t| stem(t))
                .collect()
        })
        .collect();
    let n = chunks.len() as f64;
    let avg_len = bodies.iter().map(Vec::len).sum::<usize>() as f64 / n;
    let idf: HashMap<&str, f64> = terms
        .iter()
        .map(|(term, _)| {
            let df = bodies.iter().filter(|b| b.contains(term)).count() as f64;
            (term.as_str(), (1.0 + (n - df + 0.5) / (df + 0.5)).ln())
        })
        .collect();

    let mut scored: Vec<(f64, usize)> = bodies
        .iter()
        .enumerate()
        .map(|(i, body)| {
            let len = body.len() as f64;
            let mut score = 0.0;
            for (term, weight) in &terms {
                let tf = body.iter().filter(|t| *t == term).count() as f64;
                let term_idf = idf.get(term.as_str()).copied().unwrap_or(0.0);
                if tf > 0.0 {
                    let norm = tf * (BM25_K1 + 1.0)
                        / (tf + BM25_K1 * (1.0 - BM25_B + BM25_B * len / avg_len.max(1.0)));
                    score += weight * term_idf * norm;
                }
                if headings[i].contains(term) {
                    score += weight * HEADING_BOOST * term_idf.max(0.1);
                }
            }
            (score, i)
        })
        .filter(|(score, _)| *score > 0.0)
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored
        .into_iter()
        .take(top_k.max(1))
        .map(|(score, i)| {
            let chunk = &chunks[i];
            FindHit {
                url: doc.final_url.clone(),
                text: chunk.text.clone(),
                heading_path: chunk.heading_path.clone(),
                selector: chunk.selector.clone(),
                score: Some(score),
                ..Default::default()
            }
        })
        .collect()
}

/// Matches `pattern` against the text of a document's blocks.
///
/// # Errors
///
/// Returns [`SeoError::Config`] for an invalid regex.
pub fn regex_in_document(
    doc: &PageDocument,
    pattern: &str,
    opts: &FindOptions,
) -> SeoResult<Vec<FindHit>> {
    let re = compile(pattern)?;
    Ok(regex_hits(doc, &re, opts))
}

fn compile(pattern: &str) -> SeoResult<Regex> {
    Regex::new(pattern).map_err(|e| SeoError::Config(format!("Invalid regex '{pattern}': {e}")))
}

fn regex_hits(doc: &PageDocument, re: &Regex, opts: &FindOptions) -> Vec<FindHit> {
    let mut hits = Vec::new();
    for block in &doc.blocks {
        for m in re.find_iter(&block.text) {
            if hits.len() >= opts.max_matches {
                return hits;
            }
            hits.push(FindHit {
                url: doc.final_url.clone(),
                text: m.as_str().to_string(),
                heading_path: block.heading_path.clone(),
                selector: block.selector.clone(),
                snippet: Some(context(&block.text, m.start(), m.end(), opts.context_chars)),
                ..Default::default()
            });
        }
    }
    hits
}

/// The match plus up to `chars` characters on each side, whitespace collapsed.
fn context(text: &str, start: usize, end: usize, chars: usize) -> String {
    let before: String = text[..start]
        .chars()
        .rev()
        .take(chars)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    let after: String = text[end..].chars().take(chars).collect();
    let joined = format!("{before}{}{after}", &text[start..end]);
    let mut out = joined.split_whitespace().collect::<Vec<_>>().join(" ");
    if before.len() < start {
        out.insert(0, '…');
    }
    if end + after.len() < text.len() {
        out.push('…');
    }
    out
}

/// Runs a CSS selector over a page after removing scripts and hidden elements.
///
/// # Errors
///
/// Returns [`SeoError::Config`] for an invalid selector.
pub fn select_in_html(
    html: &str,
    url: &str,
    selector: &str,
    opts: &FindOptions,
) -> SeoResult<Vec<FindHit>> {
    let dom = Document::from(html);
    clean_document(
        &dom,
        CleanOptions {
            only_main_content: false,
            exclude_selectors: &[],
        },
    )?;
    let base = url::Url::parse(url).ok();
    let selection = try_select(&dom, selector)?;
    let nodes: Vec<NodeRef> = selection
        .nodes()
        .iter()
        .copied()
        .take(opts.max_matches)
        .collect();
    let targets: HashSet<NodeId> = nodes.iter().map(|n| n.id).collect();
    let paths = heading_paths(&dom, &targets);

    Ok(nodes
        .iter()
        .map(|node| {
            let mut attributes = BTreeMap::new();
            for name in &opts.attributes {
                if let Some(value) = node.attr(name) {
                    let value = value.trim().to_string();
                    let value = match (&base, name.as_str()) {
                        (Some(base), "href" | "src" | "action" | "poster" | "data-src") => {
                            base.join(&value).map(|u| u.to_string()).unwrap_or(value)
                        }
                        _ => value,
                    };
                    attributes.insert(name.clone(), value);
                }
            }
            FindHit {
                url: url.to_string(),
                text: visible_text(node),
                heading_path: paths.get(&node.id).cloned().unwrap_or_default(),
                selector: css_path(node),
                attributes,
                html: opts.outer_html.then(|| node.html().to_string()),
                ..Default::default()
            }
        })
        .collect())
}

/// Heading path (outermost first) in effect at each target element, in document order.
fn heading_paths(dom: &Document, targets: &HashSet<NodeId>) -> HashMap<NodeId, Vec<String>> {
    let mut paths = HashMap::new();
    let mut stack: Vec<(u8, String)> = Vec::new();
    for node in dom.root().descendants_it() {
        if !node.is_element() {
            continue;
        }
        if targets.contains(&node.id) {
            paths.insert(node.id, stack.iter().map(|(_, t)| t.clone()).collect());
        }
        let name = node_name(&node);
        if let Some(level) = name
            .strip_prefix('h')
            .and_then(|l| l.parse::<u8>().ok())
            .filter(|l| (1..=6).contains(l))
        {
            while stack.last().is_some_and(|(l, _)| *l >= level) {
                stack.pop();
            }
            let text = visible_text(&node);
            if !text.is_empty() {
                stack.push((level, text));
            }
        }
    }
    paths
}

/// Fetches a page and runs the request's mode on it.
///
/// # Errors
///
/// Returns [`SeoError::Config`] when the request has no `url`, not exactly one mode, or an
/// invalid selector or regex. Page failures are reported in the result's `status`.
pub async fn find_on_page(scraper: &Scraper, req: &FindRequest) -> SeoResult<FindResult> {
    let url = req
        .url
        .as_deref()
        .ok_or_else(|| SeoError::Config("find needs a url or a crawl_id".to_string()))?;
    let mode = req.mode()?;
    let mut scrape = req.scrape.clone();
    if let Mode::Selector(_) = mode {
        scrape.formats.push(OutputFormat::RawHtml);
    }
    // Validate patterns before spending a request.
    if let Mode::Regex(pattern) = mode {
        compile(pattern)?;
    }
    if let Mode::Selector(selector) = mode {
        dom_query::Matcher::new(selector)
            .map_err(|_| SeoError::Config(format!("Invalid CSS selector '{selector}'")))?;
    }

    let doc = scraper.scrape_full(url, &scrape).await?;
    let mut result = FindResult {
        mode: mode_name(&mode).to_string(),
        url: Some(doc.final_url.clone()),
        status: Some(doc.status),
        error: doc.error.clone(),
        hits: Vec::new(),
    };
    if doc.status != PageStatus::Ok {
        return Ok(result);
    }
    result.hits = match mode {
        Mode::Query(query) => {
            find_in_document_with(&doc, query, req.options.top_k, req.options.chunk_tokens)
        }
        Mode::Regex(pattern) => regex_in_document(&doc, pattern, &req.options)?,
        Mode::Selector(selector) => match (&doc.raw_html, doc.source.as_str()) {
            (Some(html), "html" | "rendered") => {
                select_in_html(html, &doc.final_url, selector, &req.options)?
            }
            _ => {
                result.error = Some(format!(
                    "Selector mode needs an HTML page; this one was {}",
                    doc.content_type
                ));
                Vec::new()
            }
        },
    };
    Ok(result)
}

fn mode_name(mode: &Mode<'_>) -> &'static str {
    match mode {
        Mode::Query(_) => "query",
        Mode::Selector(_) => "selector",
        Mode::Regex(_) => "regex",
    }
}

/// Searches stored documents (a crawl, a host or a URL prefix). Query mode uses the FTS5
/// index; regex mode scans stored blocks. Selector mode needs live HTML and is refused.
///
/// # Errors
///
/// Returns [`SeoError::Config`] for selector mode or an invalid regex, and storage errors.
pub fn find_in_crawl(conn: &Connection, req: &FindRequest) -> SeoResult<Vec<FindHit>> {
    match req.mode()? {
        Mode::Query(query) => Ok(search_chunks(
            conn,
            &ChunkQuery {
                query: query.to_string(),
                crawl_id: req.crawl_id.clone(),
                host: req.host.clone(),
                url_prefix: req.url_prefix.clone(),
                top_k: req.options.top_k,
            },
        )?
        .into_iter()
        .map(|h| FindHit {
            url: h.url,
            text: h.text,
            heading_path: h.heading_path,
            selector: h.selector,
            score: Some(h.score),
            snippet: Some(h.snippet),
            ..Default::default()
        })
        .collect()),
        Mode::Regex(pattern) => {
            let re = compile(pattern)?;
            let crawl_id = req.crawl_id.as_deref().ok_or_else(|| {
                SeoError::Config("Regex search over stored pages needs a crawl_id".to_string())
            })?;
            let mut hits = Vec::new();
            let mut offset = 0;
            loop {
                let (docs, total) = documents_for_crawl(conn, crawl_id, offset, 100)?;
                if docs.is_empty() {
                    break;
                }
                offset += docs.len();
                for doc in docs.iter().filter(|d| {
                    d.status == PageStatus::Ok
                        && req.host.as_deref().is_none_or(|h| d.host() == h)
                        && req
                            .url_prefix
                            .as_deref()
                            .is_none_or(|p| d.final_url.starts_with(p))
                }) {
                    hits.extend(regex_hits(doc, &re, &req.options));
                }
                if offset >= total {
                    break;
                }
            }
            Ok(hits)
        }
        Mode::Selector(_) => Err(SeoError::Config(
            "Selector mode needs a url; stored crawls keep text, not HTML".to_string(),
        )),
    }
}
