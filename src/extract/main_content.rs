//! Main-content detection.
//!
//! Tried in order, first match wins:
//! 1. **selectors**: the caller's include selectors.
//! 2. **semantic**: `<main>`, `[role=main]` or a single `<article>` holding most of the text.
//! 3. **trafilatura**: rs-trafilatura picks the content; its text is mapped back onto the
//!    cleaned DOM by finding the element whose words best match (word-level F1), so our own
//!    converter still keeps tables, code blocks and links that trafilatura's output drops.
//! 4. **density**: the deepest element holding at least 80% of the page's non-link words.
//!
//! Detection sits behind [`MainContentDetector`] so the library can be swapped.

use crate::error::SeoResult;
use crate::extract::clean::{node_name, try_select};
use crate::extract::find::tokenize;
use dom_query::{Document, NodeId, NodeRef};
use hashbrown::{HashMap, HashSet};

/// Metadata a detector may learn about the page on the way.
#[derive(Debug, Clone, Default)]
pub struct DetectedMeta {
    /// Author.
    pub author: Option<String>,
    /// Publication date, RFC 3339.
    pub published: Option<String>,
    /// Site name.
    pub site_name: Option<String>,
    /// Page type (article, product, listing, forum, ...).
    pub page_type: Option<String>,
}

/// Where the main content is.
#[derive(Debug, Clone)]
pub struct MainContent {
    /// Root elements of the content, in document order.
    pub roots: Vec<NodeId>,
    /// Method name: `selectors`, `semantic`, `trafilatura`, `density` or `full`.
    pub method: &'static str,
    /// Confidence 0–1.
    pub confidence: f64,
    /// Metadata gathered during detection.
    pub meta: DetectedMeta,
}

/// A swappable main-content detector.
pub trait MainContentDetector {
    /// Finds the main content of a cleaned document. `cleaned_html` is the serialized document.
    fn detect(&self, doc: &Document, cleaned_html: &str, url: &str) -> Option<MainContent>;
}

/// rs-trafilatura with DOM anchoring.
#[derive(Debug, Default, Clone, Copy)]
pub struct TrafilaturaDetector;

impl MainContentDetector for TrafilaturaDetector {
    fn detect(&self, doc: &Document, cleaned_html: &str, url: &str) -> Option<MainContent> {
        let options = rs_trafilatura::Options {
            include_tables: true,
            include_links: false,
            include_images: false,
            url: Some(url.to_string()),
            ..Default::default()
        };
        let result = rs_trafilatura::extract_with_options(cleaned_html, &options).ok()?;
        let meta = DetectedMeta {
            author: result
                .metadata
                .author
                .clone()
                .filter(|a| !a.trim().is_empty()),
            published: result.metadata.date.map(|d| d.to_rfc3339()),
            site_name: result.metadata.sitename.clone(),
            page_type: result.metadata.page_type.clone(),
        };
        let target = tokenize(&result.content_text);
        if target.len() < 10 {
            return Some(MainContent {
                roots: Vec::new(),
                method: "trafilatura",
                confidence: 0.0,
                meta,
            });
        }
        let (node, f1) = best_matching_element(doc, &target)?;
        Some(MainContent {
            roots: if f1 >= 0.5 { vec![node] } else { Vec::new() },
            method: "trafilatura",
            confidence: (result.extraction_quality * f1).clamp(0.0, 1.0),
            meta,
        })
    }
}

/// Runs the detection chain described in the module docs.
pub fn find_main_content(
    doc: &Document,
    url: &str,
    include_selectors: &[String],
    only_main_content: bool,
    detector: &dyn MainContentDetector,
) -> SeoResult<MainContent> {
    if !include_selectors.is_empty() {
        let mut roots: Vec<NodeId> = Vec::new();
        for selector in include_selectors {
            for node in try_select(doc, selector)?.nodes() {
                // Skip nodes nested inside an already selected root.
                if !node.ancestors_it(None).any(|a| roots.contains(&a.id)) {
                    roots.push(node.id);
                }
            }
        }
        let meta = detector_meta(doc, url, detector);
        return Ok(MainContent {
            confidence: if roots.is_empty() { 0.0 } else { 1.0 },
            roots,
            method: "selectors",
            meta,
        });
    }

    let body = doc.body().map(|b| b.id);
    if !only_main_content {
        return Ok(MainContent {
            roots: body.into_iter().collect(),
            method: "full",
            confidence: 1.0,
            meta: detector_meta(doc, url, detector),
        });
    }

    let cleaned_html = doc.html().to_string();
    let detected = detector.detect(doc, &cleaned_html, url);
    let meta = detected
        .as_ref()
        .map(|d| d.meta.clone())
        .unwrap_or_default();

    let body_words = doc.body().map(|b| word_count(&b)).unwrap_or(0);
    if let Some((node, share)) = semantic_root(doc, body_words) {
        return Ok(MainContent {
            roots: vec![node],
            method: "semantic",
            confidence: (0.6 + 0.4 * share).min(0.95),
            meta,
        });
    }

    if let Some(found) = detected {
        if !found.roots.is_empty() {
            return Ok(MainContent { meta, ..found });
        }
    }

    match density_root(doc) {
        Some(node) => Ok(MainContent {
            roots: vec![node],
            method: "density",
            confidence: 0.4,
            meta,
        }),
        None => Ok(MainContent {
            roots: body.into_iter().collect(),
            method: "full",
            confidence: 0.2,
            meta,
        }),
    }
}

fn detector_meta(doc: &Document, url: &str, detector: &dyn MainContentDetector) -> DetectedMeta {
    let html = doc.html().to_string();
    detector
        .detect(doc, &html, url)
        .map(|d| d.meta)
        .unwrap_or_default()
}

fn word_count(node: &NodeRef) -> usize {
    tokenize(&node.text()).len()
}

/// `<main>` / `[role=main]`, or a lone `<article>`, when it holds at least half the words.
fn semantic_root(doc: &Document, body_words: usize) -> Option<(NodeId, f64)> {
    if body_words == 0 {
        return None;
    }
    let share = |node: &NodeRef| word_count(node) as f64 / body_words as f64;

    for selector in ["[role=main]", "main"] {
        let found = doc.select(selector);
        if found.length() == 1 {
            let node = found.nodes()[0];
            let s = share(&node);
            if s >= 0.5 && word_count(&node) >= 15 {
                return Some((node.id, s));
            }
        }
    }
    let articles = doc.select("article");
    if articles.length() == 1 {
        let node = articles.nodes()[0];
        let s = share(&node);
        if s >= 0.5 && word_count(&node) >= 15 {
            return Some((node.id, s));
        }
    }
    None
}

/// Finds the element whose word bag best matches `target` (word-level F1), in one post-order
/// pass that counts words per element.
fn best_matching_element(doc: &Document, target: &[String]) -> Option<(NodeId, f64)> {
    let mut target_counts: HashMap<&str, usize> = HashMap::new();
    for t in target {
        *target_counts.entry(t.as_str()).or_insert(0) += 1;
    }
    let target_total = target.len() as f64;
    let body = doc.body()?;

    // (matched words, total words) per element; matched is capped per word by its target count.
    let mut stats: HashMap<NodeId, (HashMap<String, usize>, usize)> = HashMap::new();
    let mut best: Option<(NodeId, f64)> = None;
    let order: Vec<NodeRef> = post_order(&body);
    for node in &order {
        let mut bag: HashMap<String, usize> = HashMap::new();
        let mut total = 0usize;
        for child in node.children_it(false) {
            if child.is_text() {
                for w in tokenize(&child.text()) {
                    total += 1;
                    if target_counts.contains_key(w.as_str()) {
                        *bag.entry(w).or_insert(0) += 1;
                    }
                }
            } else if let Some((child_bag, child_total)) = stats.remove(&child.id) {
                total += child_total;
                for (w, c) in child_bag {
                    *bag.entry(w).or_insert(0) += c;
                }
            }
        }
        let matched: usize = bag
            .iter()
            .map(|(w, c)| (*c).min(*target_counts.get(w.as_str()).unwrap_or(&0)))
            .sum();
        if total > 0 && matched > 0 {
            let precision = matched as f64 / total as f64;
            let recall = matched as f64 / target_total;
            let f1 = 2.0 * precision * recall / (precision + recall);
            if best.is_none_or(|(_, b)| f1 > b + 1e-9) {
                best = Some((node.id, f1));
            }
        }
        stats.insert(node.id, (bag, total));
    }
    best
}

fn post_order<'a>(root: &NodeRef<'a>) -> Vec<NodeRef<'a>> {
    let mut out = Vec::new();
    let mut stack: Vec<(NodeRef<'a>, bool)> = vec![(*root, false)];
    while let Some((node, visited)) = stack.pop() {
        if visited {
            out.push(node);
            continue;
        }
        stack.push((node, true));
        for child in node.children_it(true) {
            if child.is_element() {
                stack.push((child, false));
            }
        }
    }
    out
}

/// Deepest element holding at least 80% of the body's non-link words.
fn density_root(doc: &Document) -> Option<NodeId> {
    let body = doc.body()?;
    let order = post_order(&body);
    let mut words: HashMap<NodeId, usize> = HashMap::new();
    for node in &order {
        let mut total = 0usize;
        let in_link = node_name(node) == "a";
        for child in node.children_it(false) {
            if child.is_text() {
                if !in_link {
                    total += tokenize(&child.text()).len();
                }
            } else if let Some(w) = words.get(&child.id) {
                total += if in_link { 0 } else { *w };
            }
        }
        words.insert(node.id, total);
    }
    let body_total = *words.get(&body.id)?;
    if body_total == 0 {
        return None;
    }
    let threshold = (body_total as f64 * 0.8).ceil() as usize;
    let mut depth_of: HashMap<NodeId, usize> = HashMap::new();
    let mut best: Option<(NodeId, usize)> = None;
    let mut seen: HashSet<NodeId> = HashSet::new();
    for node in order.iter().rev() {
        let depth = node
            .parent()
            .and_then(|p| depth_of.get(&p.id).copied())
            .map(|d| d + 1)
            .unwrap_or(0);
        depth_of.insert(node.id, depth);
        seen.insert(node.id);
        if words.get(&node.id).copied().unwrap_or(0) >= threshold
            && best.is_none_or(|(_, d)| depth > d)
        {
            best = Some((node.id, depth));
        }
    }
    best.map(|(id, _)| id)
}
