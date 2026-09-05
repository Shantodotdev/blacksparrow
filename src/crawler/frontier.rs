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
//! ## Queue Traversal Strategies
//!
//! - **Breadth-First Search (BFS)**: Uses FIFO (`VecDeque::pop_front`). Discovers
//!   shallow, high-priority site architecture and category pages first.
//! - **Depth-First Search (DFS)**: Uses LIFO (`VecDeque::pop_back`). Traverses
//!   deep structural branches before returning.
//!
//! ## Examples
//!
//! ```rust
//! use seo_lens::crawler::frontier::{Frontier, CrawlQueueOrder};
//!
//! let mut frontier = Frontier::new(500, 3, CrawlQueueOrder::Bfs);
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
use crate::error::SeoResult;
use compact_str::CompactString;
use hashbrown::HashSet;
use std::collections::VecDeque;

/// Crawl traversal order strategy for the URL frontier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CrawlQueueOrder {
    /// Breadth-First Search (FIFO queue): Shallow pages crawled before deep pages.
    #[default]
    Bfs,
    /// Depth-First Search (LIFO stack): Deep branches explored before siblings.
    Dfs,
}

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

/// High-performance URL scheduler with SwissTable hash deduplication.
#[derive(Debug, Clone)]
pub struct Frontier {
    /// Pending candidate queue.
    queue: VecDeque<FrontierEntry>,
    /// SwissTable set containing 64-bit hashes of all scheduled/visited URLs.
    visited: HashSet<u64>,
    /// Maximum number of total unique pages to enqueue (0 = unlimited).
    max_pages: u32,
    /// Maximum crawl depth hops allowed (0 = seed only).
    max_depth: u16,
    /// Traversal order strategy (BFS vs DFS).
    order: CrawlQueueOrder,
    /// Cumulative count of successfully enqueued unique URLs.
    enqueued_count: u32,
}

impl Frontier {
    /// Creates a new `Frontier` queue with specified boundaries and ordering.
    ///
    /// # Arguments
    ///
    /// * `max_pages` - Maximum unique pages to admit (0 for unlimited).
    /// * `max_depth` - Maximum allowed link depth hops.
    /// * `order` - Traversal strategy (`Bfs` or `Dfs`).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use seo_lens::crawler::frontier::{Frontier, CrawlQueueOrder};
    ///
    /// let frontier = Frontier::new(1000, 5, CrawlQueueOrder::Bfs);
    /// assert_eq!(frontier.len(), 0);
    /// assert!(frontier.is_empty());
    /// ```
    pub fn new(max_pages: u32, max_depth: u16, order: CrawlQueueOrder) -> Self {
        Self {
            queue: VecDeque::with_capacity(128),
            visited: HashSet::with_capacity(256),
            max_pages,
            max_depth,
            order,
            enqueued_count: 0,
        }
    }

    /// Attempts to normalize, deduplicate, and enqueue a candidate URL.
    ///
    /// The candidate URL is rejected (returning `Ok(false)`) if:
    /// 1. `depth > self.max_depth`
    /// 2. The 64-bit hash of the normalized URL is already present in `visited`.
    /// 3. `self.max_pages > 0` and `enqueued_count >= self.max_pages`.
    ///
    /// # Errors
    ///
    /// Returns [`crate::error::SeoError::Url`] if the URL is syntactically invalid.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use seo_lens::crawler::frontier::{Frontier, CrawlQueueOrder};
    ///
    /// let mut frontier = Frontier::new(2, 1, CrawlQueueOrder::Bfs);
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
            return Ok(false);
        }

        if self.max_pages > 0 && self.enqueued_count >= self.max_pages {
            return Ok(false);
        }

        let normalized = normalize_url(raw_url)?;
        let hash = url_hash(&normalized);

        if !self.visited.insert(hash) {
            // Already seen/enqueued
            return Ok(false);
        }

        self.enqueued_count += 1;
        self.queue.push_back(FrontierEntry {
            url: CompactString::new(&normalized),
            depth,
            source_url: source_url.map(CompactString::new),
        });

        Ok(true)
    }

    /// Pops the next `FrontierEntry` according to the configured [`CrawlQueueOrder`].
    ///
    /// - `Bfs`: Pops from the front of the queue (FIFO).
    /// - `Dfs`: Pops from the back of the queue (LIFO).
    ///
    /// Returns `None` if the queue is empty.
    pub fn pop(&mut self) -> Option<FrontierEntry> {
        match self.order {
            CrawlQueueOrder::Bfs => self.queue.pop_front(),
            CrawlQueueOrder::Dfs => self.queue.pop_back(),
        }
    }

    /// Returns the number of currently pending URLs in the frontier queue.
    #[inline]
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    /// Returns `true` if there are no pending URLs in the frontier.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_frontier_visited_query() {
        let mut frontier = Frontier::new(50, 3, CrawlQueueOrder::Bfs);
        assert!(!frontier.is_visited("https://example.com/hello").unwrap());
        assert!(frontier.push("https://example.com/hello", 0, None).unwrap());
        assert!(frontier
            .is_visited("https://EXAMPLE.COM:443/hello?utm_source=fb")
            .unwrap());
    }
}
