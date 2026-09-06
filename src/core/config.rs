//! # Crawl Configuration
//!
//! Strongly-typed configuration options governing crawler execution,
//! politeness delays, depth boundaries, concurrency, and rendering mode.
//!
//! ## Politeness & Congestion Control
//!
//! A polite crawler balances audit throughput with origin server health:
//! - **Concurrency**: Number of simultaneous async green tasks fetching pages (default 10).
//! - **Politeness Delay**: When `delay_ms == 0`, the crawler automatically applies an
//!   Additive-Increase/Multiplicative-Decrease (AIMD) congestion control algorithm to adapt
//!   to server response latencies and avoid triggering rate limits or 429 Too Many Requests.
//! - **Robots Compliance**: Respects `/robots.txt` disallow rules per RFC 9309 unless explicitly disabled.
//!
//! ## JavaScript Rendering Mode
//!
//! By default, SEO Lens executes fast streaming HTTP requests without browser overhead.
//! When `render_js` is enabled, the crawler connects to a decoupled Headless Chrome instance
//! via Chrome DevTools Protocol (CDP) to evaluate Client-Side Rendered (CSR) applications.

use crate::core::url::normalize_url;
use crate::error::{SeoError, SeoResult};
use serde::{Deserialize, Serialize};

/// Default User-Agent string used by SEO Lens.
pub const DEFAULT_USER_AGENT: &str = "SEOLens/1.0";

/// Configuration parameters governing a crawl session.
///
/// Controls start URLs, boundaries (max pages, max depth), concurrency,
/// politeness delays, HTTP headers, proxy routing, and rendering modes.
///
/// # Examples
///
/// ```rust
/// use seo_lens::core::config::CrawlConfig;
///
/// let mut config = CrawlConfig::new("https://example.com").unwrap();
/// config.max_pages = 1000;
/// config.concurrency = 16;
/// config.render_js = true;
///
/// assert!(config.validate().is_ok());
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlConfig {
    /// Starting root URL (automatically normalized on creation).
    pub start_url: String,
    /// Maximum number of unique pages to crawl (0 = unlimited).
    pub max_pages: u32,
    /// Maximum link hops from start URL (0 = start URL only).
    pub max_depth: u16,
    /// Number of concurrent asynchronous fetch tasks.
    pub concurrency: usize,
    /// Fixed delay between requests in milliseconds (0 = dynamic AIMD congestion control).
    pub delay_ms: u64,
    /// Enable Headless Chrome CDP for client-side JavaScript rendering.
    pub render_js: bool,
    /// Remote Chrome WebSocket URL (None = auto-launch local Chrome).
    pub chrome_ws: Option<String>,
    /// Custom User-Agent header string.
    pub user_agent: String,
    /// Whether to strictly respect `/robots.txt` disallow directives.
    pub respect_robots: bool,
    /// Optional HTTP/HTTPS/SOCKS5 proxy URL for outbound requests.
    pub proxy: Option<String>,
    /// Custom HTTP request headers to include with every request.
    pub headers: Vec<(String, String)>,
    /// If true, do not persist results to SQLite; auto-cleanup on finish.
    pub ephemeral: bool,
    /// Disable dynamic AIMD rate throttling (useful for high-speed local/staging site crawls).
    pub no_aimd: bool,
    /// Maximum number of content query parameters before flagging or pruning faceted spider traps (default: 2).
    pub max_query_params: usize,
    /// Whether to prune sorting and display facet URLs (e.g. ?sort=, ?order=, ?view=) (default: true).
    pub ignore_sorting_facets: bool,
}

impl CrawlConfig {
    /// Creates a new `CrawlConfig` with normalized start URL and safe defaults.
    ///
    /// Defaults:
    /// - `max_pages`: 500
    /// - `max_depth`: 5
    /// - `concurrency`: 10
    /// - `delay_ms`: 0 (dynamic AIMD politeness)
    /// - `user_agent`: "SEOLens/1.0"
    /// - `respect_robots`: true
    /// - `max_query_params`: 2
    /// - `ignore_sorting_facets`: true
    ///
    /// # Errors
    ///
    /// Returns [`SeoError::Url`] if `start_url` cannot be parsed or normalized.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use seo_lens::core::config::CrawlConfig;
    ///
    /// let config = CrawlConfig::new("HTTPS://EXAMPLE.COM/").unwrap();
    /// assert_eq!(config.start_url, "https://example.com/");
    /// assert_eq!(config.concurrency, 10);
    /// assert!(config.respect_robots);
    /// assert_eq!(config.max_query_params, 2);
    /// assert!(config.ignore_sorting_facets);
    /// ```
    pub fn new(start_url: &str) -> SeoResult<Self> {
        let normalized = normalize_url(start_url)?;
        Ok(Self {
            start_url: normalized,
            max_pages: 500,
            max_depth: 5,
            concurrency: 10,
            delay_ms: 0,
            render_js: false,
            chrome_ws: None,
            user_agent: DEFAULT_USER_AGENT.to_string(),
            respect_robots: true,
            proxy: None,
            headers: Vec::new(),
            ephemeral: false,
            no_aimd: false,
            max_query_params: 2,
            ignore_sorting_facets: true,
        })
    }

    /// Validates configuration parameters for semantic correctness.
    ///
    /// Ensures concurrency $\ge 1$ and non-empty user-agent string.
    ///
    /// # Errors
    ///
    /// Returns [`SeoError::Config`] if any configuration parameter violates constraints.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use seo_lens::core::config::CrawlConfig;
    ///
    /// let mut config = CrawlConfig::new("https://example.com").unwrap();
    /// assert!(config.validate().is_ok());
    ///
    /// config.concurrency = 0;
    /// assert!(config.validate().is_err());
    /// ```
    pub fn validate(&self) -> SeoResult<()> {
        if self.concurrency == 0 {
            return Err(SeoError::Config(
                "Concurrency must be at least 1".to_string(),
            ));
        }
        if self.user_agent.trim().is_empty() {
            return Err(SeoError::Config("User-Agent cannot be empty".to_string()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_crawl_config_defaults_and_validation() {
        let config = CrawlConfig::new("https://example.com/").unwrap();
        assert_eq!(config.start_url, "https://example.com/");
        assert_eq!(config.max_pages, 500);
        assert_eq!(config.max_depth, 5);
        assert_eq!(config.concurrency, 10);
        assert_eq!(config.user_agent, DEFAULT_USER_AGENT);
        assert!(config.respect_robots);
        assert_eq!(config.max_query_params, 2);
        assert!(config.ignore_sorting_facets);
        assert!(config.validate().is_ok());

        let mut invalid_config = config.clone();
        invalid_config.concurrency = 0;
        assert!(matches!(
            invalid_config.validate(),
            Err(SeoError::Config(_))
        ));

        let mut empty_ua = config.clone();
        empty_ua.user_agent = "   ".to_string();
        assert!(matches!(empty_ua.validate(), Err(SeoError::Config(_))));
    }
}
