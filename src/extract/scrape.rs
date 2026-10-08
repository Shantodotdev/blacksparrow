//! `scrape`: one URL in, one clean [`PageDocument`] out.
//!
//! Pipeline, cheapest first:
//! 1. Network guard on the URL (private addresses and cloud metadata are refused).
//! 2. Cache: a stored copy younger than `max_age` is returned without a request.
//! 3. robots.txt (per-host cache).
//! 4. HTTP fetch asking for Markdown first (`Accept: text/markdown, text/html;q=0.9`).
//!    Markdown, plain text and PDF responses are converted directly; challenge pages,
//!    oversized and non-text responses are classified rather than returned as content.
//! 5. Chrome only when the options require it or the raw HTML is an empty app shell.

use crate::core::url::validate_url_safety;
use crate::crawler::client::{FetchOptions, FetchResult, HttpClient};
use crate::crawler::render_pool::{RenderOutput, RenderPool, RenderPoolConfig, RenderRequest};
use crate::crawler::robots::RobotsTxt;
use crate::error::SeoResult;
use crate::extract::document::{html_to_document, markdown_to_document, text_to_document};
use crate::extract::pdf::pdf_to_document;
use crate::extract::types::{
    unix_now, OutputFormat, PageDocument, PageStatus, RenderMode, ScrapeOptions,
};
use crate::storage::documents::{latest_document, save_document};
use crate::storage::Database;
use base64::Engine;
use hashbrown::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, OnceCell};

/// `Accept` header for the Markdown fast path.
pub const ACCEPT_MARKDOWN: &str = "text/markdown, text/html;q=0.9, text/plain;q=0.8, */*;q=0.5";

/// Scraper settings.
#[derive(Debug, Clone)]
pub struct ScraperConfig {
    /// User-Agent for HTTP and Chrome.
    pub user_agent: String,
    /// Per-request timeout.
    pub timeout: Duration,
    /// Extra request headers.
    pub headers: Vec<(String, String)>,
    /// Outbound proxy.
    pub proxy: Option<String>,
    /// Allow private and local addresses (cloud metadata stays blocked).
    pub allow_all_private_ips: bool,
    /// Specific private `host` / `host:port` entries allowed.
    pub allowed_private_hosts: Vec<String>,
    /// Obey robots.txt.
    pub respect_robots: bool,
    /// Remote Chrome CDP endpoint (`None` launches a local Chrome on first use).
    pub chrome_ws: Option<String>,
    /// Maximum tabs rendering at once.
    pub render_concurrency: usize,
    /// SQLite database for caching and change tracking (`None` = no storage).
    pub db_path: Option<PathBuf>,
}

impl Default for ScraperConfig {
    fn default() -> Self {
        Self {
            user_agent: crate::core::branding::DEFAULT_USER_AGENT.to_string(),
            timeout: Duration::from_secs(30),
            headers: Vec::new(),
            proxy: None,
            allow_all_private_ips: false,
            allowed_private_hosts: Vec::new(),
            respect_robots: true,
            chrome_ws: None,
            render_concurrency: 4,
            db_path: None,
        }
    }
}

/// Reusable scraper: one HTTP client, a robots.txt cache, a lazily started browser pool and
/// optional storage.
pub struct Scraper {
    client: HttpClient,
    config: ScraperConfig,
    robots: Mutex<HashMap<String, Option<RobotsTxt>>>,
    renderer: OnceCell<Option<Arc<RenderPool>>>,
    db: Option<Database>,
}

impl std::fmt::Debug for Scraper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scraper")
            .field("config", &self.config)
            .finish()
    }
}

impl Scraper {
    /// Creates a scraper.
    ///
    /// # Errors
    ///
    /// Returns an error when the HTTP client cannot be built or the database cannot be opened.
    pub fn new(config: ScraperConfig) -> SeoResult<Self> {
        let client = HttpClient::new(FetchOptions {
            user_agent: config.user_agent.clone(),
            timeout: config.timeout,
            custom_headers: config.headers.clone(),
            proxy: config.proxy.clone(),
            allow_all_private_ips: config.allow_all_private_ips,
            allowed_private_hosts: config.allowed_private_hosts.clone(),
            ..Default::default()
        })?;
        let db = match &config.db_path {
            Some(path) => Some(Database::open(path)?),
            None => None,
        };
        Ok(Self {
            client,
            config,
            robots: Mutex::new(HashMap::new()),
            renderer: OnceCell::new(),
            db,
        })
    }

    /// Creates a scraper that shares an already running browser pool.
    ///
    /// # Errors
    ///
    /// Same as [`Scraper::new`].
    pub fn with_renderer(config: ScraperConfig, renderer: Arc<RenderPool>) -> SeoResult<Self> {
        let scraper = Self::new(config)?;
        let _ = scraper.renderer.set(Some(renderer));
        Ok(scraper)
    }

    /// The settings this scraper was built with.
    pub fn config(&self) -> &ScraperConfig {
        &self.config
    }

    /// The underlying HTTP client.
    pub fn client(&self) -> &HttpClient {
        &self.client
    }

    /// The storage this scraper caches into, if any.
    pub fn database(&self) -> Option<&Database> {
        self.db.as_ref()
    }

    /// Scrapes one URL and returns the document shaped to `opts.formats`. Page-level failures
    /// (blocked, too large, network errors) are reported in the document's status, not as `Err`.
    ///
    /// # Errors
    ///
    /// Returns an error only for invalid options (for example a bad CSS selector).
    pub async fn scrape(&self, url: &str, opts: &ScrapeOptions) -> SeoResult<PageDocument> {
        let mut doc = self.scrape_full(url, opts).await?;
        doc.apply_formats(opts);
        Ok(doc)
    }

    /// Like [`Scraper::scrape`], storing the full document under `crawl_id` and returning it
    /// without applying formats.
    ///
    /// # Errors
    ///
    /// Returns an error only for invalid options.
    pub async fn scrape_full(&self, url: &str, opts: &ScrapeOptions) -> SeoResult<PageDocument> {
        self.scrape_into(url, opts, None).await
    }

    /// Scrapes and stores the document with a crawl id.
    ///
    /// # Errors
    ///
    /// Returns an error only for invalid options.
    pub async fn scrape_into(
        &self,
        url: &str,
        opts: &ScrapeOptions,
        crawl_id: Option<&str>,
    ) -> SeoResult<PageDocument> {
        let mut doc = self.scrape_unsaved(url, opts).await?;
        self.store(&mut doc, crawl_id);
        Ok(doc)
    }

    /// Runs the scrape pipeline without writing to storage (the cache is still read).
    ///
    /// # Errors
    ///
    /// Returns an error only for invalid options.
    pub async fn scrape_unsaved(&self, url: &str, opts: &ScrapeOptions) -> SeoResult<PageDocument> {
        if let Err(e) = validate_url_safety(
            url,
            self.config.allow_all_private_ips,
            &self.config.allowed_private_hosts,
        )
        .await
        {
            return Ok(error_doc(url, &e.to_string()));
        }

        if let (Some(max_age), Some(db)) = (opts.max_age_secs, &self.db) {
            if let Some(mut cached) = self.cached(db, url, max_age) {
                cached.from_cache = true;
                return Ok(cached);
            }
        }

        if self.config.respect_robots && !self.robots_allows(url).await {
            let mut doc = PageDocument::with_status(url, PageStatus::Blocked);
            doc.blocked_by = Some("robots.txt".to_string());
            doc.error = Some("Disallowed by robots.txt".to_string());
            return Ok(doc);
        }

        self.fetch_document(url, opts).await
    }

    /// Stores a freshly fetched document (cached copies and pages that were never requested
    /// are skipped) and sets its `changed` flag.
    pub fn store(&self, doc: &mut PageDocument, crawl_id: Option<&str>) {
        if doc.from_cache || doc.status_code == 0 {
            return;
        }
        if let Some(db) = &self.db {
            if let Ok(conn) = db.connect() {
                if let Ok(stored) = save_document(&conn, crawl_id, doc) {
                    doc.changed = stored.changed;
                }
            }
        }
    }

    fn cached(&self, db: &Database, url: &str, max_age: u64) -> Option<PageDocument> {
        let conn = db.connect().ok()?;
        let (doc, fetched_at) = latest_document(&conn, url).ok()??;
        (unix_now().saturating_sub(fetched_at) <= max_age).then_some(doc)
    }

    async fn fetch_document(&self, url: &str, opts: &ScrapeOptions) -> SeoResult<PageDocument> {
        if opts.requires_browser() {
            return self.render_document(url, opts, None).await;
        }

        let fetched = if opts.accept_markdown {
            self.client
                .fetch_with_headers(url, &[("Accept", ACCEPT_MARKDOWN)])
                .await
        } else {
            self.client.fetch(url).await
        };
        let res = match fetched {
            Ok(res) => res,
            Err(e) => return Ok(error_doc(url, &e.to_string())),
        };

        let mut doc = self.classify(url, &res, opts)?;
        if doc.status == PageStatus::Ok
            && doc.source == "html"
            && opts.render == RenderMode::Auto
            && looks_like_app_shell(&res.body, &doc)
        {
            if let Ok(rendered) = self.render_document(url, opts, Some(&res)).await {
                if rendered.status == PageStatus::Ok {
                    doc = rendered;
                }
            }
        }
        Ok(doc)
    }

    /// Turns a raw HTTP response into a document or an honest failure status.
    fn classify(
        &self,
        url: &str,
        res: &FetchResult,
        opts: &ScrapeOptions,
    ) -> SeoResult<PageDocument> {
        let content_type = res
            .content_type
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        let base = |status: PageStatus| {
            let mut d = PageDocument::with_status(url, status);
            d.final_url = res.final_url.clone();
            d
        };

        let mut doc = if let Some(vendor) = res.waf_detected {
            let mut d = base(PageStatus::Blocked);
            d.blocked_by = Some(vendor.to_string());
            d.error = Some(format!(
                "{vendor} served a bot challenge instead of the page"
            ));
            d
        } else if res.is_truncated {
            let mut d = base(PageStatus::TooLarge);
            d.error = Some(format!(
                "Response exceeded the {} byte limit",
                crate::crawler::client::DEFAULT_MAX_RESPONSE_BYTES
            ));
            d
        } else if content_type == "application/pdf" || res.body_bytes.starts_with(b"%PDF-") {
            pdf_to_document(&res.body_bytes, &res.final_url, opts)
        } else if content_type == "text/markdown" || content_type == "text/x-markdown" {
            markdown_to_document(&res.body, &res.final_url, opts)
        } else if crate::crawler::engine::is_html_document(&res.content_type, &res.body) {
            html_to_document(&res.body, &res.final_url, opts)?
        } else if content_type == "text/plain" {
            text_to_document(&res.body, &res.final_url, opts)
        } else {
            let mut d = base(PageStatus::NotHtml);
            d.error = Some(format!("Unsupported content type '{content_type}'"));
            d
        };

        doc.url = url.to_string();
        doc.final_url = res.final_url.clone();
        doc.status_code = res.status_code;
        doc.content_type = content_type;
        doc.metadata.content_signal = res
            .headers
            .get("content-signal")
            .and_then(|v| v.to_str().ok())
            .map(str::to_string);
        if opts.wants(OutputFormat::RawHtml) {
            doc.raw_html = Some(res.body.clone());
        }
        if doc.status == PageStatus::Ok && res.status_code >= 400 {
            doc.error = Some(format!("HTTP {}", res.status_code));
        }
        Ok(doc)
    }

    async fn renderer(&self) -> Option<Arc<RenderPool>> {
        self.renderer
            .get_or_init(|| async {
                RenderPool::launch(RenderPoolConfig {
                    chrome_ws: self.config.chrome_ws.clone(),
                    max_tabs: self.config.render_concurrency,
                    user_agent: self.config.user_agent.clone(),
                    headers: self.config.headers.clone(),
                    proxy: self.config.proxy.clone(),
                    allow_all_private_ips: self.config.allow_all_private_ips,
                    allowed_private_hosts: self.config.allowed_private_hosts.clone(),
                    block_resources: true,
                })
                .await
                .ok()
                .map(Arc::new)
            })
            .await
            .clone()
    }

    /// Renders `url` in Chrome and converts the DOM. `raw` carries the HTTP response when one
    /// was already fetched (auto mode), so its status code is kept.
    async fn render_document(
        &self,
        url: &str,
        opts: &ScrapeOptions,
        raw: Option<&FetchResult>,
    ) -> SeoResult<PageDocument> {
        let Some(pool) = self.renderer().await else {
            return Ok(error_doc(
                url,
                "Chrome is not available: install Chrome/Chromium or pass a CDP endpoint",
            ));
        };
        let request = RenderRequest {
            wait_until: opts.wait_until.clone(),
            wait_for_selector: opts.wait_for_selector.clone(),
            wait_ms: opts.wait_ms,
            actions: opts.actions.clone(),
            screenshot: opts.wants(OutputFormat::Screenshot),
            snapshot: false,
            timeout: opts.timeout_ms.map(Duration::from_millis),
        };
        let output = match pool.render(url, &request).await {
            Ok(output) => output,
            Err(e) => return Ok(error_doc(url, &e.to_string())),
        };
        rendered_to_document(url, &output, opts, raw)
    }

    /// Renders `url` with an explicit request (used by `interact`).
    ///
    /// # Errors
    ///
    /// Returns an error when Chrome is unavailable or rendering fails.
    pub async fn render_raw(&self, url: &str, request: &RenderRequest) -> SeoResult<RenderOutput> {
        validate_url_safety(
            url,
            self.config.allow_all_private_ips,
            &self.config.allowed_private_hosts,
        )
        .await?;
        if self.config.respect_robots && !self.robots_allows(url).await {
            return Err(crate::error::SeoError::Network(format!(
                "Disallowed by robots.txt: {url}"
            )));
        }
        let pool = self.renderer().await.ok_or_else(|| {
            crate::error::SeoError::Network(
                "Chrome is not available: install Chrome/Chromium or pass a CDP endpoint"
                    .to_string(),
            )
        })?;
        pool.render(url, request).await
    }

    /// Checks robots.txt for `url` with this scraper's user agent (cached per origin).
    pub async fn robots_allows(&self, url: &str) -> bool {
        let Ok(parsed) = url::Url::parse(url) else {
            return false;
        };
        let origin = parsed.origin().ascii_serialization();
        let mut cache = self.robots.lock().await;
        if !cache.contains_key(&origin) {
            let robots = match self.client.fetch(&format!("{origin}/robots.txt")).await {
                Ok(res) if res.status_code == 200 => Some(RobotsTxt::parse(&res.body)),
                // RFC 9309: an unreachable robots.txt (5xx) means "assume disallowed".
                Ok(res) if res.status_code >= 500 => {
                    Some(RobotsTxt::parse("User-agent: *\nDisallow: /\n"))
                }
                _ => None,
            };
            cache.insert(origin.clone(), robots);
        }
        match cache.get(&origin) {
            Some(Some(robots)) => robots.is_allowed(&self.config.user_agent, url),
            _ => true,
        }
    }
}

/// Converts a rendered page into a document.
pub fn rendered_to_document(
    url: &str,
    output: &RenderOutput,
    opts: &ScrapeOptions,
    raw: Option<&FetchResult>,
) -> SeoResult<PageDocument> {
    let mut doc = html_to_document(&output.html, &output.final_url, opts)?;
    doc.url = url.to_string();
    doc.final_url = output.final_url.clone();
    doc.source = "rendered".to_string();
    doc.status_code = raw.map(|r| r.status_code).unwrap_or(200);
    doc.content_type = "text/html".to_string();
    if let Some(png) = &output.screenshot_png {
        doc.screenshot = Some(base64::engine::general_purpose::STANDARD.encode(png));
    }
    if opts.wants(OutputFormat::RawHtml) {
        doc.raw_html = Some(output.html.clone());
    }
    if !output.action_errors.is_empty() {
        doc.error = Some(output.action_errors.join("; "));
    }
    Ok(doc)
}

fn error_doc(url: &str, message: &str) -> PageDocument {
    let mut doc = PageDocument::with_status(url, PageStatus::Error);
    doc.error = Some(message.to_string());
    doc
}

/// The existing SPA check: an empty app root, almost no text and no links.
fn looks_like_app_shell(body: &str, doc: &PageDocument) -> bool {
    let words = doc.text.split_whitespace().count();
    let has_root = [
        "id=\"root\"",
        "id=\"app\"",
        "id=\"__next\"",
        "id=\"__nuxt\"",
        "id=\"svelte\"",
    ]
    .iter()
    .any(|marker| crate::core::url::contains_ignore_ascii_case(body, marker));
    has_root && words < 50 && doc.links.is_empty()
}
