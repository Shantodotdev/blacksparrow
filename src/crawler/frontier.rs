//! # Crawl Frontier & Scheduling Queue
//!
//! High-performance URL discovery queue and visited-set deduplication engine.
//!
//! ## Architecture & SwissTable Deduplication
//!
//! The crawler frontier is responsible for scheduling candidate URLs, managing
//! crawl depth boundaries, and preventing infinite crawl loops.
//!
//! To achieve high throughput and minimal memory consumption during large-scale
//! audits (50,000+ pages), URLs are normalized through [`crate::core::url::normalize_url`]
//! and hashed into 64-bit AHash identifiers ([`crate::core::url::url_hash`]).
//!
//! Deduplication is maintained in a SwissTable [`hashbrown::HashSet<u64>`],
//! which requires only 8 bytes per visited URL—yielding an order-of-magnitude
//! memory reduction over storing raw strings:
//!
//! $$\text{Memory}(50{,}000\text{ URLs}) \approx 50{,}000 \times 8\text{ bytes} \approx 400\text{ KB}$$
//!
//! ## Intelligent Importance Scheduling
//!
//! The frontier uses an importance-weighted priority queue backed by [`std::collections::BinaryHeap`].
//! Candidate URLs are scored using structural importance ($S_{\text{seed}}$, $S_{\text{depth}}$,
//! $S_{\text{indegree}}$, $S_{\text{path}}$), query parameter penalties ($P_{\text{query}}$), and
//! pagination deprioritization ($P_{\text{pagination}}$) via [`calculate_url_importance`].
//!
//! ## Examples
//!
//! ```rust
//! use seo_lens::crawler::frontier::Frontier;
//!
//! let mut frontier = Frontier::new(500, 3);
//!
//! // Enqueue root URL at depth 0
//! assert!(frontier.push("https://example.com", 0, None).unwrap());
//!
//! // Normalization deduplicates query tracking and trailing slashes
//! assert!(!frontier.push("https://example.com/?utm_source=test", 1, None).unwrap());
//!
//! let next_page = frontier.pop().unwrap();
//! assert_eq!(next_page.url, "https://example.com/");
//! assert_eq!(next_page.depth, 0);
//! ```

use crate::core::url::{normalize_url, url_hash};
use crate::crawler::priority::calculate_url_importance;
use crate::error::SeoResult;
use compact_str::CompactString;
use hashbrown::{HashMap, HashSet};
use std::collections::BinaryHeap;

/// A scheduled crawl target popped from the frontier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontierEntry {
    /// Fully normalized target URL.
    pub url: CompactString,
    /// Crawl hop distance from the seed/root URL (root = 0).
    pub depth: u16,
    /// Normalized URL of the page where this link was discovered.
    pub source_url: Option<CompactString>,
}

/// Wrapper for scheduling URLs in a max-heap prioritized by importance score.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrioritizedEntry {
    /// Importance score $P(u)$.
    pub priority: i64,
    /// Tie-breaking sequence counter (lower sequence = earlier discovery).
    pub sequence: u64,
    /// 64-bit URL hash for fast SwissTable lookup.
    pub url_hash: u64,
    /// The wrapped frontier entry.
    pub entry: FrontierEntry,
}

impl Ord for PrioritizedEntry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.priority
            .cmp(&other.priority)
            // Tie-break: lower sequence number wins (FIFO among identical scores)
            .then_with(|| other.sequence.cmp(&self.sequence))
    }
}

impl PartialOrd for PrioritizedEntry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// High-performance URL scheduler with SwissTable hash deduplication.
#[derive(Debug, Clone)]
pub struct Frontier {
    /// Priority heap for importance-based traversal.
    priority_heap: BinaryHeap<PrioritizedEntry>,
    /// SwissTable set containing 64-bit hashes of all scheduled/visited URLs.
    visited: HashSet<u64>,
    /// SwissTable set of URLs currently waiting in the frontier queue (for deduplication and in-flight updates).
    pending_urls: HashSet<u64>,
    /// In-degree incoming reference counter per URL hash for authority accumulation.
    in_degrees: HashMap<u64, u32>,
    /// 64-bit hashes of declared XML sitemap URLs for seed boost calculation.
    sitemap_hashes: HashSet<u64>,
    /// Monotonically increasing sequence number for deterministic tie-breaking.
    sequence_counter: u64,
    /// Maximum number of total unique pages to enqueue (0 = unlimited).
    max_pages: u32,
    /// Maximum crawl depth hops allowed (0 = seed only).
    max_depth: u16,
    /// Cumulative count of successfully enqueued unique URLs.
    enqueued_count: u32,
    /// Whether any candidate URL was rejected because max_pages limit was reached.
    hit_max_pages: bool,
    /// Whether any candidate URL was rejected because max_depth limit was exceeded.
    hit_max_depth: bool,
}

impl Frontier {
    /// Creates a new `Frontier` priority queue with specified boundaries.
    ///
    /// # Arguments
    ///
    /// * `max_pages` - Maximum unique pages to admit (0 for unlimited).
    /// * `max_depth` - Maximum allowed link depth hops.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use seo_lens::crawler::frontier::Frontier;
    ///
    /// let frontier = Frontier::new(1000, 5);
    /// assert_eq!(frontier.len(), 0);
    /// assert!(frontier.is_empty());
    /// ```
    pub fn new(max_pages: u32, max_depth: u16) -> Self {
        Self {
            priority_heap: BinaryHeap::with_capacity(128),
            visited: HashSet::with_capacity(256),
            pending_urls: HashSet::with_capacity(128),
            in_degrees: HashMap::with_capacity(128),
            sitemap_hashes: HashSet::with_capacity(64),
            sequence_counter: 0,
            max_pages,
            max_depth,
            enqueued_count: 0,
            hit_max_pages: false,
            hit_max_depth: false,
        }
    }

    /// Registers a collection of XML sitemap URLs to receive seed priority boosts.
    pub fn register_sitemap_urls(&mut self, urls: &[String]) {
        for url in urls {
            if let Ok(normalized) = normalize_url(url) {
                self.sitemap_hashes.insert(url_hash(&normalized));
            }
        }
    }

    /// Attempts to normalize, deduplicate, and enqueue a candidate URL.
    ///
    /// The candidate URL is rejected (returning `Ok(false)`) if:
    /// 1. `depth > self.max_depth`
    /// 2. The 64-bit hash of the normalized URL is already present in `visited`.
    /// 3. `self.max_pages > 0` and `enqueued_count >= self.max_pages`.
    ///
    /// If the URL is already pending in the queue, its in-degree reference count
    /// is incremented and an updated entry with boosted priority is pushed to the heap.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::SeoError::Url`] if the URL is syntactically invalid.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use seo_lens::crawler::frontier::Frontier;
    ///
    /// let mut frontier = Frontier::new(2, 1);
    /// assert!(frontier.push("https://example.com/page1", 1, None).unwrap());
    ///
    /// // Duplicate URL
    /// assert!(!frontier.push("https://example.com/page1", 1, None).unwrap());
    ///
    /// // Depth limit exceeded (depth 2 > max_depth 1)
    /// assert!(!frontier.push("https://example.com/page2", 2, None).unwrap());
    /// ```
    pub fn push(&mut self, raw_url: &str, depth: u16, source_url: Option<&str>) -> SeoResult<bool> {
        if depth > self.max_depth {
            self.hit_max_depth = true;
            return Ok(false);
        }

        let normalized = normalize_url(raw_url)?;
        let hash = url_hash(&normalized);

        // Dynamic in-degree authority accumulation:
        // If the candidate URL is already pending in the queue, increment its reference count
        // and re-push a prioritized entry with escalated importance!
        if self.pending_urls.contains(&hash) {
            let count = self.in_degrees.entry(hash).or_insert(1);
            *count = count.saturating_add(1);
            let in_degree = *count;

            let is_sitemap = self.sitemap_hashes.contains(&hash);
            let priority = calculate_url_importance(&normalized, depth, in_degree, is_sitemap);
            self.sequence_counter += 1;
            self.priority_heap.push(PrioritizedEntry {
                priority,
                sequence: self.sequence_counter,
                url_hash: hash,
                entry: FrontierEntry {
                    url: CompactString::new(&normalized),
                    depth,
                    source_url: source_url.map(CompactString::new),
                },
            });
            return Ok(false);
        }

        if self.max_pages > 0 && self.enqueued_count >= self.max_pages {
            self.hit_max_pages = true;
            return Ok(false);
        }

        if !self.visited.insert(hash) {
            // Already visited / crawled
            return Ok(false);
        }

        self.enqueued_count += 1;
        self.pending_urls.insert(hash);
        self.in_degrees.insert(hash, 1);
        self.sequence_counter += 1;

        let entry = FrontierEntry {
            url: CompactString::new(&normalized),
            depth,
            source_url: source_url.map(CompactString::new),
        };

        let is_sitemap = self.sitemap_hashes.contains(&hash);
        let priority = calculate_url_importance(&normalized, depth, 1, is_sitemap);
        self.priority_heap.push(PrioritizedEntry {
            priority,
            sequence: self.sequence_counter,
            url_hash: hash,
            entry,
        });

        Ok(true)
    }

    /// Pops the next highest-scoring `FrontierEntry` from the priority heap.
    ///
    /// Lazily prunes stale duplicate entries whose URL was already popped.
    /// Returns `None` if the queue is empty.
    pub fn pop(&mut self) -> Option<FrontierEntry> {
        while let Some(prioritized) = self.priority_heap.pop() {
            if self.pending_urls.remove(&prioritized.url_hash) {
                return Some(prioritized.entry);
            }
        }
        None
    }

    /// Returns the number of currently pending unique URLs in the frontier queue.
    #[inline]
    pub fn len(&self) -> usize {
        self.pending_urls.len()
    }

    /// Returns `true` if there are no pending URLs in the frontier.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.pending_urls.is_empty()
    }

    /// Checks whether a given URL has already been visited or enqueued.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::SeoError::Url`] if the URL cannot be normalized.
    pub fn is_visited(&self, raw_url: &str) -> SeoResult<bool> {
        let normalized = normalize_url(raw_url)?;
        let hash = url_hash(&normalized);
        Ok(self.visited.contains(&hash))
    }

    /// Returns the total number of unique URLs admitted and stored in the visited set.
    #[inline]
    pub fn visited_count(&self) -> usize {
        self.visited.len()
    }

    /// Returns the cumulative number of unique URLs that were enqueued.
    #[inline]
    pub fn enqueued_count(&self) -> u32 {
        self.enqueued_count
    }

    /// Returns `true` if any candidate URL was rejected due to the max pages limit.
    #[inline]
    pub fn hit_max_pages(&self) -> bool {
        self.hit_max_pages
    }

    /// Returns `true` if any candidate URL was rejected due to the max depth limit.
    #[inline]
    pub fn hit_max_depth(&self) -> bool {
        self.hit_max_depth
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frontier_visited_query() {
        let mut frontier = Frontier::new(50, 3);
        assert!(!frontier.is_visited("https://example.com/hello").unwrap());
        assert!(frontier.push("https://example.com/hello", 0, None).unwrap());
        assert!(frontier
            .is_visited("https://EXAMPLE.COM:443/hello?utm_source=fb")
            .unwrap());
    }
}
