//! # Crawl Configuration
//!
//! Strongly-typed configuration options governing crawler execution,
//! politeness delays, depth boundaries, concurrency, and rendering mode.

use crate::core::url::normalize_url;
use crate::error::{SeoError, SeoResult};
use serde::{Deserialize, Serialize};

/// Default User-Agent string used by SEO Lens.
pub const DEFAULT_USER_AGENT: &str = "SEOLens/1.0";

/// Configuration parameters governing a crawl session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlConfig {
    /// Starting root URL (normalized)
    pub start_url: String,
    /// Maximum number of pages to crawl (0 = unlimited)
    pub max_pages: u32,
    /// Maximum crawl depth from start URL
    pub max_depth: u16,
    /// Number of concurrent fetch tasks
    pub concurrency: usize,
    /// Fixed delay between requests in milliseconds (0 = dynamic AIMD)
    pub delay_ms: u64,
    /// Enable Headless Chrome CDP for client-side JavaScript rendering
    pub render_js: bool,
    /// Remote Chrome WebSocket URL (None = auto-launch local Chrome)
    pub chrome_ws: Option<String>,
    /// Custom User-Agent header string
    pub user_agent: String,
    /// Whether to strictly respect /robots.txt directives
    pub respect_robots: bool,
    /// Optional HTTP/HTTPS/SOCKS5 proxy URL
    pub proxy: Option<String>,
    /// Custom HTTP headers to include with requests
    pub headers: Vec<(String, String)>,
    /// If true, do not persist results to SQLite; auto-cleanup on finish
    pub ephemeral: bool,
}

impl CrawlConfig {
    /// Creates a new `CrawlConfig` with normalized start URL and safe defaults.
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
        })
    }

    /// Validates the configuration parameters.
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
