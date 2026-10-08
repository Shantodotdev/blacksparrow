//! Content crawl: scrape many pages of one site into clean documents.
//!
//! The crawl seeds its frontier with the start URL and (by default) the site's sitemaps, then
//! follows same-site links breadth-first up to `max_depth`. Every page goes through the same
//! [`Scraper`] pipeline as a single `scrape` (network guard, cache, robots.txt, Markdown fast
//! path, Chrome when needed). Request pacing reuses the SEO crawler's AIMD controller.
//! Cross-page boilerplate is removed before documents reach the sink.

use crate::core::url::is_static_asset_url;
use crate::crawler::aimd::AimdController;
use crate::crawler::engine::{politeness_wait, record_politeness};
use crate::error::{SeoError, SeoResult};
use crate::extract::boilerplate::BoilerplateFilter;
use crate::extract::document::finalize;
use crate::extract::map::{canonical_key, SiteScope, SitemapMode};
use crate::extract::paths::PathFilter;
use crate::extract::scrape::Scraper;
use crate::extract::sink::PageSink;
use crate::extract::types::{PageDocument, PageStatus, ScrapeOptions};
use futures::stream::{FuturesUnordered, StreamExt};
use hashbrown::{HashMap, HashSet};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex as StdMutex;
use tokio::sync::Mutex;

/// Options for [`crawl_site`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CrawlOptions {
    /// Maximum pages scraped (successful or not).
    pub limit: usize,
    /// Maximum link depth from the start URL (sitemap URLs count as depth 1). `0` follows
    /// no links.
    #[serde(alias = "maxDepth", alias = "maxDiscoveryDepth")]
    pub max_depth: u16,
    /// Only scrape URLs whose path matches one of these patterns (the start URL is always
    /// scraped). See [`PathFilter`].
    #[serde(alias = "includePaths")]
    pub include_paths: Vec<String>,
    /// Never scrape URLs whose path matches one of these patterns.
    #[serde(alias = "excludePaths")]
    pub exclude_paths: Vec<String>,
    /// Sitemap usage.
    pub sitemap: SitemapMode,
    /// Follow links to subdomains of the start host.
    #[serde(alias = "allowSubdomains")]
    pub allow_subdomains: bool,
    /// Treat URLs that differ only in their query string as one page.
    #[serde(alias = "ignoreQueryParameters")]
    pub ignore_query: bool,
    /// Pages fetched at once.
    pub concurrency: usize,
    /// Fixed delay between requests in milliseconds (`0` = adaptive AIMD pacing).
    #[serde(alias = "delay")]
    pub delay_ms: u64,
    /// Remove text repeated on most pages of the site.
    #[serde(alias = "deduplicateBoilerplate")]
    pub dedupe_boilerplate: bool,
    /// Per-page scrape options.
    #[serde(alias = "scrapeOptions")]
    pub scrape: ScrapeOptions,
}

impl Default for CrawlOptions {
    fn default() -> Self {
        Self {
            limit: 100,
            max_depth: 5,
            include_paths: Vec::new(),
            exclude_paths: Vec::new(),
            sitemap: SitemapMode::Include,
            allow_subdomains: false,
            ignore_query: false,
            concurrency: 4,
            delay_ms: 0,
            dedupe_boilerplate: true,
            scrape: ScrapeOptions::default(),
        }
    }
}

/// Lifecycle of a crawl.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CrawlState {
    /// Created, not started.
    #[default]
    Queued,
    /// Running.
    Crawling,
    /// Finished normally.
    Completed,
    /// Stopped by a cancel request.
    Cancelled,
    /// Stopped by an error.
    Failed,
}

impl CrawlState {
    /// Lowercase name, as stored.
    pub fn as_str(self) -> &'static str {
        match self {
            CrawlState::Queued => "queued",
            CrawlState::Crawling => "crawling",
            CrawlState::Completed => "completed",
            CrawlState::Cancelled => "cancelled",
            CrawlState::Failed => "failed",
        }
    }

    /// Parses a stored name.
    pub fn parse(s: &str) -> Self {
        match s {
            "crawling" => CrawlState::Crawling,
            "completed" => CrawlState::Completed,
            "cancelled" => CrawlState::Cancelled,
            "failed" => CrawlState::Failed,
            _ => CrawlState::Queued,
        }
    }

    /// Whether the crawl has stopped.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            CrawlState::Completed | CrawlState::Cancelled | CrawlState::Failed
        )
    }
}

/// Live counters of a crawl.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CrawlProgress {
    /// Current state.
    pub state: CrawlState,
    /// Pages scraped with status `ok`.
    pub pages_done: u32,
    /// Pages that failed, were blocked or were not HTML.
    pub pages_failed: u32,
    /// URLs skipped by robots.txt.
    pub pages_skipped: u32,
    /// Distinct URLs queued so far.
    pub discovered: u32,
    /// Error that stopped the crawl.
    pub error: Option<String>,
}

/// Shared handle to observe and cancel a running crawl.
#[derive(Debug, Default)]
pub struct CrawlControl {
    cancelled: AtomicBool,
    progress: StdMutex<CrawlProgress>,
}

impl CrawlControl {
    /// Asks the crawl to stop; pages already being fetched finish first.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    /// Whether cancellation was requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// A snapshot of the counters.
    pub fn progress(&self) -> CrawlProgress {
        self.progress.lock().map(|p| p.clone()).unwrap_or_default()
    }

    fn update(&self, f: impl FnOnce(&mut CrawlProgress)) {
        if let Ok(mut p) = self.progress.lock() {
            f(&mut p);
        }
    }
}

/// Crawls the site at `url`, writing each document to `sink`, and storing documents under
/// `crawl_id` when the scraper has a database.
///
/// # Errors
///
/// Returns an error for an invalid start URL, invalid options, or a sink write failure. The
/// control's state is set to `failed` in that case.
pub async fn crawl_site(
    scraper: &Scraper,
    url: &str,
    opts: &CrawlOptions,
    sink: &mut dyn PageSink,
    control: &CrawlControl,
) -> SeoResult<()> {
    crawl_site_with_id(scraper, url, opts, sink, control, None).await
}

/// [`crawl_site`] with a crawl id attached to stored documents.
///
/// # Errors
///
/// Same as [`crawl_site`].
pub async fn crawl_site_with_id(
    scraper: &Scraper,
    url: &str,
    opts: &CrawlOptions,
    sink: &mut dyn PageSink,
    control: &CrawlControl,
    crawl_id: Option<&str>,
) -> SeoResult<()> {
    control.update(|p| p.state = CrawlState::Crawling);
    let outcome = run(scraper, url, opts, sink, control, crawl_id).await;
    let state = match &outcome {
        Err(_) => CrawlState::Failed,
        Ok(()) if control.is_cancelled() => CrawlState::Cancelled,
        Ok(()) => CrawlState::Completed,
    };
    control.update(|p| {
        p.state = state;
        if let Err(e) = &outcome {
            p.error = Some(e.to_string());
        }
    });
    outcome
}

struct Frontier {
    queue: VecDeque<(String, u16)>,
    seen: HashSet<String>,
}

async fn run(
    scraper: &Scraper,
    url: &str,
    opts: &CrawlOptions,
    sink: &mut dyn PageSink,
    control: &CrawlControl,
    crawl_id: Option<&str>,
) -> SeoResult<()> {
    let seed =
        url::Url::parse(url).map_err(|e| SeoError::Url(format!("Invalid URL '{url}': {e}")))?;
    let filter = PathFilter::new(&opts.include_paths, &opts.exclude_paths)?;
    let scope = SiteScope::new(&seed, opts.allow_subdomains);
    let seed_key = canonical_key(url, opts.ignore_query)
        .ok_or_else(|| SeoError::Url(format!("Invalid URL '{url}'")))?;
    let concurrency = opts.concurrency.max(1);
    let aimd = Mutex::new(AimdController::new(concurrency, 0));

    let mut frontier = Frontier {
        queue: VecDeque::new(),
        seen: HashSet::new(),
    };
    frontier.seen.insert(seed_key.clone());
    frontier.queue.push_back((seed_key, 0));
    if control.is_cancelled() {
        return Ok(());
    }

    if opts.sitemap != SitemapMode::Skip {
        let (_, sitemap_urls, _) = crate::crawler::engine::discover_robots_and_sitemaps(
            scraper.client(),
            url,
            false,
            &[],
            opts.limit.min(u32::MAX as usize) as u32,
        )
        .await;
        for u in sitemap_urls {
            enqueue(&mut frontier, &scope, &filter, opts, &u, 1);
        }
    }
    control.update(|p| p.discovered = frontier.seen.len() as u32);

    let mut boilerplate: HashMap<String, BoilerplateFilter> = HashMap::new();
    let mut pending: HashMap<String, Vec<PageDocument>> = HashMap::new();
    let mut scheduled = 0usize;
    let mut in_flight = FuturesUnordered::new();

    loop {
        while !control.is_cancelled() && scheduled < opts.limit {
            let cap = aimd
                .lock()
                .await
                .current_concurrency()
                .clamp(1, concurrency);
            if in_flight.len() >= cap {
                break;
            }
            let Some((next, depth)) = frontier.queue.pop_front() else {
                break;
            };
            if scraper.config().respect_robots && !scraper.robots_allows(&next).await {
                control.update(|p| p.pages_skipped += 1);
                continue;
            }
            scheduled += 1;
            let aimd = &aimd;
            let delay = opts.delay_ms;
            let scrape = &opts.scrape;
            in_flight.push(async move {
                politeness_wait(aimd, false, delay).await;
                let doc = scraper.scrape_unsaved(&next, scrape).await;
                let outcome = match &doc {
                    Ok(d) if d.status_code > 0 => Some((d.status_code, 0)),
                    _ => None,
                };
                if !matches!(&doc, Ok(d) if d.from_cache) {
                    record_politeness(aimd, false, delay, outcome).await;
                }
                (doc, depth)
            });
        }

        let Some((doc, depth)) = in_flight.next().await else {
            break;
        };
        let mut doc = doc?;
        if doc.status == PageStatus::Ok {
            control.update(|p| p.pages_done += 1);
        } else {
            control.update(|p| p.pages_failed += 1);
        }

        if opts.sitemap != SitemapMode::Only && depth < opts.max_depth && !control.is_cancelled() {
            for link in &doc.outlinks {
                enqueue(&mut frontier, &scope, &filter, opts, &link.url, depth + 1);
            }
            control.update(|p| p.discovered = frontier.seen.len() as u32);
        }

        if !opts.dedupe_boilerplate {
            emit(scraper, sink, &mut doc, opts, crawl_id)?;
            continue;
        }
        let host = doc.host();
        let learner = boilerplate.entry(host.clone()).or_default();
        if learner.is_ready() {
            strip(learner, &mut doc, opts);
            emit(scraper, sink, &mut doc, opts, crawl_id)?;
        } else {
            learner.observe(&doc);
            pending.entry(host.clone()).or_default().push(doc);
            if learner.is_ready() {
                for mut buffered in pending.remove(&host).unwrap_or_default() {
                    strip(learner, &mut buffered, opts);
                    emit(scraper, sink, &mut buffered, opts, crawl_id)?;
                }
            }
        }
    }

    for (host, docs) in pending {
        let learner = boilerplate.entry(host).or_default();
        for mut doc in docs {
            strip(learner, &mut doc, opts);
            emit(scraper, sink, &mut doc, opts, crawl_id)?;
        }
    }
    sink.finish()
}

fn enqueue(
    frontier: &mut Frontier,
    scope: &SiteScope,
    filter: &PathFilter,
    opts: &CrawlOptions,
    url: &str,
    depth: u16,
) {
    if !scope.contains(url) || is_static_asset_url(url) {
        return;
    }
    let Some(key) = canonical_key(url, opts.ignore_query) else {
        return;
    };
    if !filter.allows(&key) || !frontier.seen.insert(key.clone()) {
        return;
    }
    frontier.queue.push_back((key, depth));
}

fn strip(learner: &BoilerplateFilter, doc: &mut PageDocument, opts: &CrawlOptions) {
    if !doc.from_cache && learner.strip(doc) {
        finalize(doc, &opts.scrape);
    }
}

fn emit(
    scraper: &Scraper,
    sink: &mut dyn PageSink,
    doc: &mut PageDocument,
    opts: &CrawlOptions,
    crawl_id: Option<&str>,
) -> SeoResult<()> {
    scraper.store(doc, crawl_id);
    doc.apply_formats(&opts.scrape);
    sink.write(doc)
}
