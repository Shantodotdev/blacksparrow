//! Cross-page boilerplate removal for crawls.
//!
//! Template text that survives single-page cleaning (promo strips, repeated disclaimers,
//! "related" teasers inside `<main>`) shows up as identical blocks on most pages of a site.
//! The filter samples the first pages of each host and, once it has seen enough, drops any
//! block that appeared on more than half of the sampled pages. Page titles (`h1`) are never
//! removed.

use crate::extract::types::{Block, BlockKind, PageDocument, PageStatus};
use hashbrown::{HashMap, HashSet};

/// Pages sampled per host before stripping starts.
pub const DEFAULT_SAMPLE_PAGES: usize = 20;

/// Fewer sampled pages than this and nothing is ever treated as boilerplate.
const MIN_SAMPLE_PAGES: usize = 4;

/// Learns repeated blocks for one host.
#[derive(Debug, Clone)]
pub struct BoilerplateFilter {
    sample_pages: usize,
    sampled: usize,
    counts: HashMap<u64, usize>,
}

impl Default for BoilerplateFilter {
    fn default() -> Self {
        Self::new(DEFAULT_SAMPLE_PAGES)
    }
}

impl BoilerplateFilter {
    /// Creates a filter that samples `sample_pages` pages.
    pub fn new(sample_pages: usize) -> Self {
        Self {
            sample_pages: sample_pages.max(1),
            sampled: 0,
            counts: HashMap::new(),
        }
    }

    /// Counts the blocks of a page while the sample is still being collected.
    pub fn observe(&mut self, doc: &PageDocument) {
        if self.is_ready() || !eligible(doc) {
            return;
        }
        self.sampled += 1;
        let keys: HashSet<u64> = doc.blocks.iter().filter_map(block_key).collect();
        for key in keys {
            *self.counts.entry(key).or_insert(0) += 1;
        }
    }

    /// Whether the sample is complete.
    pub fn is_ready(&self) -> bool {
        self.sampled >= self.sample_pages
    }

    /// Removes learned boilerplate blocks from `doc`. Returns whether anything was removed.
    pub fn strip(&self, doc: &mut PageDocument) -> bool {
        if self.sampled < MIN_SAMPLE_PAGES || !eligible(doc) {
            return false;
        }
        let keep: Vec<bool> = doc
            .blocks
            .iter()
            .map(|b| match block_key(b) {
                Some(key) => self.counts.get(&key).copied().unwrap_or(0) * 2 <= self.sampled,
                None => true,
            })
            .collect();
        // Never strip a page down to nothing, and leave untouched pages alone.
        if keep.iter().all(|k| *k) || !keep.iter().any(|k| *k) {
            return false;
        }
        let mut flags = keep.into_iter();
        doc.blocks.retain(|_| flags.next().unwrap_or(true));
        true
    }
}

fn eligible(doc: &PageDocument) -> bool {
    doc.status == PageStatus::Ok && matches!(doc.source.as_str(), "html" | "rendered")
}

fn block_key(block: &Block) -> Option<u64> {
    if matches!(block.kind, BlockKind::Heading { level: 1 }) {
        return None;
    }
    let normalized = block
        .text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase();
    if normalized.is_empty() {
        return None;
    }
    let kind = match &block.kind {
        BlockKind::Heading { .. } => "h",
        BlockKind::Paragraph => "p",
        BlockKind::List { .. } => "l",
        BlockKind::Table { .. } => "t",
        BlockKind::Code { .. } => "c",
        BlockKind::Quote => "q",
        BlockKind::Image { .. } => "i",
    };
    Some(crate::core::url::url_hash(&format!(
        "{kind}\u{1f}{normalized}"
    )))
}
