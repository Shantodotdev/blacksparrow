//! # Multi-Page Asynchronous Crawl Engine
//!
//! Coordinates the end-to-end technical SEO audit workflow:
//! 1. **Discovery**: Fetches `robots.txt`, identifies AI crawler restrictions, probes `/llms.txt`, and parses recursive XML sitemaps.
//! 2. **Frontier Scheduling**: Manages URL prioritization, depth limits, deduplication, and query parameter filtering.
//! 3. **Concurrent Execution**: Spawns Tokio green tasks bounded by an asynchronous semaphore and rate-limited by an adaptive AIMD controller.
//! 4. **Streaming Processing**: Parses HTML in a single streaming pass (`lol_html`) and evaluates 120 technical SEO rules.
//! 5. **Real-Time Persistence**: Streams page reports and findings into SQLite WAL storage via an asynchronous batch actor.
//! 6. **Graceful Cancellation**: Traps SIGINT (`Ctrl+C`) and cancellation channels to drain in-flight workers and persist partial crawls cleanly.
//! 7. **Topology & Scoring**: Constructs a petgraph directed link graph, computes internal PageRank, detects whole-site graph issues, and produces the health scorecard.

use crate::core::config::CrawlConfig;
use crate::core::models::{DiscoveredLink, IssueFinding, PageReport, RuleId, Severity};
use crate::core::url::{
    count_content_facets, has_sorting_facets, is_internal, is_static_asset_url, normalize_url,
    url_hash,
};
use crate::crawler::aimd::AimdController;
use crate::crawler::client::{FetchOptions, FetchResult, HttpClient};
use crate::crawler::frontier::{Frontier, FrontierEntry};
use crate::crawler::robots::RobotsTxt;
use crate::crawler::sitemap::{parse_sitemap, SitemapDocument};
use crate::error::SeoResult;
use crate::graph::{compute_pagerank, SiteGraph};
use crate::parser::{parse_html, ParsedPage};
use crate::report::score::calculate_health_score;
use crate::rules::catalog::get_rule;
use crate::rules::{evaluate_graph, evaluate_page};
use crate::storage::DbWriterHandle;
use compact_str::CompactString;
use hashbrown::{HashMap, HashSet};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Mutex, Semaphore};

/// Real-time progress update telemetry emitted during crawl execution.
#[derive(Debug, Clone)]
pub struct ProgressUpdate {
    /// Total number of unique pages successfully fetched and audited.
    pub crawled_pages: usize,
    /// Total count of unique URLs discovered so far (crawled + pending in frontier).
    pub discovered_pages: usize,
    /// Configured maximum page limit (0 indicates unlimited crawl).
    pub max_pages: u32,
    /// Normalized URL of the most recently processed page.
    pub current_url: String,
    /// HTTP status code returned by the most recent page fetch.
    pub status_code: u16,
    /// Time to first byte (TTFB) latency in milliseconds for the most recent page.
    pub ttfb_ms: u32,
    /// Current dynamic inter-request delay imposed by the AIMD congestion controller in milliseconds.
    pub aimd_delay_ms: u64,
    /// Cumulative count of Critical severity defects detected across all pages.
    pub critical_count: usize,
    /// Cumulative count of Alert severity defects detected across all pages.
    pub alert_count: usize,
    /// Cumulative count of Warning severity defects detected across all pages.
    pub warning_count: usize,
}

/// Thread-safe callback closure invoked with live telemetry as pages are processed.
pub type ProgressCallback = Arc<dyn Fn(ProgressUpdate) + Send + Sync>;

/// Complete aggregated output of a multi-page technical SEO audit.
#[derive(Debug, Clone)]
pub struct CrawlResult {
    /// Seed start URL that initiated the crawl.
    pub target_url: String,
    /// Individual audited page reports for all successfully crawled URLs.
    pub pages: Vec<PageReport>,
    /// Directed hyperlink topology graph representing site architecture.
    pub graph: SiteGraph,
    /// Internal PageRank authority scores keyed by 64-bit URL hash.
    pub pagerank: HashMap<u64, f64>,
    /// All technical SEO defects detected (both single-page and post-crawl graph rules).
    pub issues: Vec<IssueFinding>,
    /// Total wall-clock time elapsed during crawl execution.
    pub duration: Duration,
    /// URLs discovered and registered from XML sitemaps.
    pub sitemap_urls: Vec<String>,
    /// Final dynamic delay in milliseconds calculated by the AIMD controller.
    pub aimd_delay_ms: u64,
    /// Overall technical health score on a normalized 0–100 scale.
    pub health_score: u8,
}

#[derive(Debug)]
struct WorkerPageOutcome {
    report: PageReport,
    discovered_links: Vec<DiscoveredLink>,
    depth: u16,
}

fn is_html_document(content_type: &str, body: &str) -> bool {
    let ct = content_type.to_lowercase();
    if ct.contains("text/html") || ct.contains("application/xhtml+xml") {
        return true;
    }
    if ct.is_empty() || ct.contains("text/plain") {
        let trimmed = body.trim_start();
        return trimmed.starts_with("<!DOCTYPE")
            || trimmed.starts_with("<!doctype")
            || trimmed.starts_with("<html")
            || trimmed.starts_with("<HTML")
            || trimmed.starts_with("<head")
            || trimmed.starts_with("<HEAD");
    }
    false
}

fn build_page_report(
    session_id: &str,
    url: &str,
    depth: u16,
    res: &FetchResult,
    parsed: Option<&ParsedPage>,
    issues: Vec<IssueFinding>,
) -> PageReport {
    let mut report = PageReport {
        crawl_id: CompactString::new(session_id),
        url: url.to_string(),
        url_hash: url_hash(url),
        final_url: Some(res.final_url.clone()),
        status_code: res.status_code,
        content_type: CompactString::new(&res.content_type),
        size_bytes: res.size_bytes,
        ttfb_ms: res.ttfb_ms,
        crawl_depth: depth,
        is_internal: true,
        has_lorem_ipsum: res.body.to_lowercase().contains("lorem ipsum"),
        is_https: url.starts_with("https://"),
        has_hsts: res.headers.contains_key("strict-transport-security"),
        has_csp: res.headers.contains_key("content-security-policy"),
        has_x_frame: res.headers.contains_key("x-frame-options"),
        has_x_content_type: res.headers.contains_key("x-content-type-options"),
        issues,
        ..Default::default()
    };

    if let Some(p) = parsed {
        report.title = p.title.clone();
        report.title_length = p.title.as_ref().map(|t| t.len() as u16).unwrap_or(0);
        report.meta_description = p.meta_description.clone();
        report.meta_desc_length = p
            .meta_description
            .as_ref()
            .map(|d| d.len() as u16)
            .unwrap_or(0);
        report.canonical_url = p.canonical_url.clone();
        report.html_lang = p.html_lang.clone();
        report.charset = p.charset.clone();
        report.viewport = p.viewport.clone();
        report.robots_flags = p.robots_flags;
        report.h1_primary = p.h1_primary.clone();
        report.h1_count = p.h1_count;
        report.h2_headings = p.h2_headings.clone();
        report.h3_headings = p.h3_headings.clone();
        report.word_count = p.word_count;
        report.content_hash = p.content_hash;
        report.simhash = p.simhash;
        report.links = p.links.clone();
        report.images = p.images.clone();
        report.schemas = p.schemas.clone();
        report.hreflangs = p.hreflangs.clone();
        report.page_intent = p.page_intent.clone();
    }

    report
}

/// Discovers and parses robots.txt and XML sitemaps before crawling starts.
///
/// 1. Probes `/robots.txt` and extracts sitemap declarations.
/// 2. Audits AI search crawler disallows (`GPTBot`, `ClaudeBot`, etc.) per Rule 11.1.
/// 3. Checks for presence of `/llms.txt` per Rule 11.2.
/// 4. If no sitemaps are declared in robots.txt, falls back to probing convention paths
///    (`/sitemap.xml`, `/sitemap_index.xml`, `/wp-sitemap.xml`) per CRAWLER_SPEC §5.2.
/// 5. Recursively resolves nested sitemap index feeds up to 3 levels deep.
async fn discover_robots_and_sitemaps(
    client: &HttpClient,
    seed_url: &str,
    respect_robots: bool,
) -> (Option<RobotsTxt>, Vec<String>, Vec<IssueFinding>) {
    let Ok(parsed_url) = url::Url::parse(seed_url) else {
        return (None, Vec::new(), Vec::new());
    };

    let origin = format!("{}://{}", parsed_url.scheme(), parsed_url.authority());
    let robots_url = format!("{}/robots.txt", origin);
    let mut sitemap_feed_seeds = Vec::new();
    let mut site_issues = Vec::new();

    let fetched_robots = if let Ok(res) = client.fetch(&robots_url).await {
        if res.status_code == 200 {
            let parsed_robots = RobotsTxt::parse(&res.body);
            for sm in parsed_robots.sitemaps() {
                sitemap_feed_seeds.push(sm.to_string());
            }

            // Inspect for AI search citation bots disallow (Category 11)
            const AI_SEARCH_BOTS: &[&str] = &[
                "GPTBot",
                "ClaudeBot",
                "PerplexityBot",
                "CCBot",
                "OAI-SearchBot",
            ];

            let mut blocked_ai_bots = Vec::new();
            for &bot in AI_SEARCH_BOTS {
                if !parsed_robots.is_allowed(bot, "/") {
                    blocked_ai_bots.push(bot);
                }
            }

            if !blocked_ai_bots.is_empty() {
                let rule = get_rule(RuleId::AlertAiSearchBotsBlocked);
                let msg = format!(
                    "Robots.txt disallows AI search and citation crawlers ({}), preventing discovery in AI search overviews.",
                    blocked_ai_bots.join(", ")
                );
                site_issues.push(rule.to_finding(&robots_url, Some(&msg)));
            }

            Some(parsed_robots)
        } else {
            None
        }
    } else {
        None
    };

    let robots = if respect_robots { fetched_robots } else { None };

    // Probe /llms.txt (Category 11)
    let llms_url = format!("{}/llms.txt", origin);
    let has_llms_txt = match client.fetch(&llms_url).await {
        Ok(res) => res.status_code == 200,
        Err(_) => false,
    };

    if !has_llms_txt {
        let rule = get_rule(RuleId::WarnLlmsTxtMissing);
        let msg = format!(
            "The website does not publish a /llms.txt file at {}.",
            llms_url
        );
        site_issues.push(rule.to_finding(&llms_url, Some(&msg)));
    }

    // If robots.txt declared no sitemaps, probe standard conventions per CRAWLER_SPEC §5.2
    if sitemap_feed_seeds.is_empty() {
        sitemap_feed_seeds.push(format!("{}/sitemap.xml", origin));
        sitemap_feed_seeds.push(format!("{}/sitemap_index.xml", origin));
        sitemap_feed_seeds.push(format!("{}/wp-sitemap.xml", origin));
    }

    // Recursively fetch and parse XML sitemaps up to 3 levels deep
    let mut queue = VecDeque::new();
    let mut visited_feeds = HashSet::new();
    let mut discovered_pages = HashSet::new();

    for feed in sitemap_feed_seeds {
        queue.push_back((feed, 0u8));
    }

    while let Some((feed_url, depth)) = queue.pop_front() {
        if depth > 3 || !visited_feeds.insert(feed_url.clone()) {
            continue;
        }

        if let Ok(res) = client.fetch(&feed_url).await {
            if res.status_code == 200 {
                if let Ok(doc) = parse_sitemap(&res.body_bytes) {
                    match doc {
                        SitemapDocument::UrlSet(entries) => {
                            for entry in entries {
                                let loc = entry.loc.as_str();
                                if is_internal(loc, &origin)
                                    && !is_static_asset_url(loc)
                                    && !loc.ends_with(".xml")
                                    && !loc.ends_with(".xml.gz")
                                {
                                    if let Ok(norm) = normalize_url(loc) {
                                        discovered_pages.insert(norm);
                                    } else {
                                        discovered_pages.insert(loc.to_string());
                                    }
                                }
                            }
                        }
                        SitemapDocument::Index(sub_sitemaps) => {
                            if depth < 3 {
                                for sub in sub_sitemaps {
                                    let child_feed = sub.loc.as_str().to_string();
                                    if is_internal(&child_feed, &origin)
                                        && !visited_feeds.contains(&child_feed)
                                    {
                                        queue.push_back((child_feed, depth + 1));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    let sitemap_urls = discovered_pages.into_iter().collect::<Vec<String>>();
    (robots, sitemap_urls, site_issues)
}

/// Fetches a single frontier entry, applies AIMD politeness throttling, evaluates single-page rules, and generates a PageReport.
///
/// Feeds TTFB latency metrics and HTTP response status codes back into the AIMD controller
/// to dynamically optimize request throughput without overloading the target origin.
async fn fetch_and_audit_page(
    session_id: &str,
    entry: FrontierEntry,
    client: Arc<HttpClient>,
    aimd: Arc<Mutex<AimdController>>,
    no_aimd: bool,
    static_delay: u64,
) -> Option<WorkerPageOutcome> {
    if !no_aimd && static_delay == 0 {
        let delay_ms = {
            let a = aimd.lock().await;
            a.current_delay_ms()
        };
        if delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        }
    } else if static_delay > 0 {
        tokio::time::sleep(Duration::from_millis(static_delay)).await;
    }

    let url_str = entry.url.as_str();
    let fetch_res = client.fetch(url_str).await;

    match fetch_res {
        Ok(res) => {
            if !no_aimd && static_delay == 0 {
                let mut a = aimd.lock().await;
                if res.status_code >= 400 {
                    a.record_failure(Some(res.status_code), res.status_code == 429);
                } else {
                    a.record_success(res.ttfb_ms);
                }
            }

            let parsed = if is_html_document(&res.content_type, &res.body) {
                parse_html(&res.body, &res.final_url).ok()
            } else {
                None
            };

            let page_issues = if let Some(ref p) = parsed {
                evaluate_page(p, &res)
            } else {
                Vec::new()
            };

            let discovered_links = parsed.as_ref().map(|p| p.links.clone()).unwrap_or_default();
            let depth = entry.depth;

            let report = build_page_report(
                session_id,
                url_str,
                depth,
                &res,
                parsed.as_ref(),
                page_issues,
            );

            Some(WorkerPageOutcome {
                report,
                discovered_links,
                depth,
            })
        }
        Err(_) => {
            if !no_aimd && static_delay == 0 {
                let mut a = aimd.lock().await;
                a.record_failure(None, false);
            }
            None
        }
    }
}

/// Constructs the directed site graph, computes PageRank, evaluates whole-site graph rules, and calculates the health score.
///
/// Executed after the crawl loop terminates (either upon exhaustive discovery, hitting max page limits, or cancellation).
fn finalize_crawl(
    normalized_start: String,
    mut pages: Vec<PageReport>,
    mut issues: Vec<IssueFinding>,
    sitemap_urls: Vec<String>,
    aimd_delay_ms: u64,
    elapsed: Duration,
    crawl_exhaustive: bool,
) -> CrawlResult {
    {
        let sitemap_set: HashSet<&str> = sitemap_urls.iter().map(|s| s.as_str()).collect();
        for page in &mut pages {
            if sitemap_set.contains(page.url.as_str()) {
                page.is_sitemap_url = true;
            }
        }
    }

    let graph = SiteGraph::from_pages(&pages, &sitemap_urls);
    let pagerank = compute_pagerank(&graph, 0.85, 100, 1e-6);

    let graph_issues = evaluate_graph(&pages, &graph, &sitemap_urls, crawl_exhaustive, &pagerank);
    issues.extend(graph_issues);

    let health_score = calculate_health_score(pages.len(), &issues);

    CrawlResult {
        target_url: normalized_start,
        pages,
        graph,
        pagerank,
        issues,
        duration: elapsed,
        sitemap_urls,
        aimd_delay_ms,
        health_score,
    }
}

/// Executes a full technical SEO crawl using default in-memory channels.
///
/// Dispatches concurrent workers up to the configured concurrency limit,
/// evaluates SEO rules on each page, and aggregates the final audit matrix.
///
/// # Arguments
///
/// * `config` - Crawl options controlling target URL, concurrency, depth limits, and politeness.
/// * `progress_cb` - Optional closure invoked as each page completes to update progress bars or UI.
///
/// # Errors
///
/// Returns [`SeoError`](crate::error::SeoError) if the seed URL is invalid or the HTTP client fails initialization.
pub async fn run_crawl(
    config: &CrawlConfig,
    progress_cb: Option<ProgressCallback>,
) -> SeoResult<CrawlResult> {
    run_crawl_with_options(config, progress_cb, None, None).await
}

/// Executes a full technical SEO crawl with support for real-time SQLite streaming and cooperative cancellation.
///
/// # Concurrency & Graceful Shutdown
///
/// - Worker green tasks are governed by a [`tokio::sync::Semaphore`] set to `config.concurrency`.
/// - If a SIGINT (`Ctrl+C`) or `cancel_rx` signal is received, the engine halts dispatch of new URLs,
///   allows in-flight worker tasks up to 2 seconds to complete and save their results,
///   updates the session status to `interrupted`, and flushes all pending records to SQLite.
///
/// # Arguments
///
/// * `config` - Crawl configuration options.
/// * `progress_cb` - Optional callback invoked with live telemetry updates.
/// * `db_writer` - Optional actor handle to stream pages and issues directly into SQLite WAL storage.
/// * `cancel_rx` - Optional receiver for cooperative external cancellation (e.g. from desktop UI or test).
///
/// # Errors
///
/// Returns [`SeoError`](crate::error::SeoError) if URL normalization or network client initialization fails.
pub async fn run_crawl_with_options(
    config: &CrawlConfig,
    progress_cb: Option<ProgressCallback>,
    db_writer: Option<DbWriterHandle>,
    mut cancel_rx: Option<tokio::sync::oneshot::Receiver<()>>,
) -> SeoResult<CrawlResult> {
    let start_time = Instant::now();
    let normalized_start = normalize_url(&config.start_url)?;

    let session_id = config.session_id.clone().unwrap_or_else(|| {
        format!(
            "crawl_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs()
        )
    });

    let client = Arc::new(HttpClient::new(FetchOptions {
        user_agent: config.user_agent.clone(),
        timeout: Duration::from_secs(30),
        max_redirects: 10,
        ..Default::default()
    })?);

    let (robots_txt, sitemap_urls, site_issues) =
        discover_robots_and_sitemaps(&client, &normalized_start, config.respect_robots).await;

    if let Some(ref writer) = db_writer {
        let _ = writer.save_issues(site_issues.clone()).await;
    }

    let frontier = Arc::new(Mutex::new(Frontier::new(
        config.max_pages,
        config.max_depth,
    )));

    {
        let mut f = frontier.lock().await;
        f.register_sitemap_urls(&sitemap_urls);
        f.push(&normalized_start, 0, None)?;
    }

    let aimd = Arc::new(Mutex::new(AimdController::new(
        config.concurrency,
        config.delay_ms,
    )));

    let concurrency = config.concurrency.max(1);
    let semaphore = Arc::new(Semaphore::new(concurrency));
    let active_workers = Arc::new(AtomicUsize::new(0));

    let (tx, mut rx) = mpsc::channel::<WorkerPageOutcome>(concurrency * 2);

    let mut crawled_pages = Vec::new();
    let mut all_issues = Vec::new();
    let mut critical_count = 0usize;
    let mut alert_count = 0usize;
    let mut warning_count = 0usize;
    let mut hit_max_pages = false;
    let mut interrupted = false;
    let mut ctrl_c_stream = Box::pin(tokio::signal::ctrl_c());

    for issue in &site_issues {
        match issue.severity {
            Severity::Critical => critical_count += 1,
            Severity::Alert => alert_count += 1,
            Severity::Warning => warning_count += 1,
            Severity::Notice => {}
        }
        all_issues.push(issue.clone());
    }

    loop {
        if interrupted {
            break;
        }

        while let Ok(permit) = semaphore.clone().try_acquire_owned() {
            if interrupted {
                drop(permit);
                break;
            }

            let next_entry = {
                let mut f = frontier.lock().await;
                if config.max_pages > 0
                    && (crawled_pages.len() + active_workers.load(Ordering::SeqCst))
                        >= config.max_pages as usize
                {
                    None
                } else {
                    f.pop()
                }
            };

            match next_entry {
                Some(entry) => {
                    if let Some(ref robots) = robots_txt {
                        if !robots.is_allowed(&config.user_agent, &entry.url) {
                            drop(permit);
                            continue;
                        }
                    }

                    active_workers.fetch_add(1, Ordering::SeqCst);

                    let client_clone = Arc::clone(&client);
                    let aimd_clone = Arc::clone(&aimd);
                    let tx_clone = tx.clone();
                    let no_aimd = config.no_aimd;
                    let static_delay = config.delay_ms;
                    let sess_id = session_id.clone();

                    tokio::spawn(async move {
                        let _permit = permit;
                        if let Some(outcome) = fetch_and_audit_page(
                            &sess_id,
                            entry,
                            client_clone,
                            aimd_clone,
                            no_aimd,
                            static_delay,
                        )
                        .await
                        {
                            let _ = tx_clone.send(outcome).await;
                        }
                    });
                }
                None => {
                    drop(permit);
                    break;
                }
            }
        }

        if config.max_pages > 0 && crawled_pages.len() >= config.max_pages as usize {
            hit_max_pages = true;
            break;
        }

        let active = active_workers.load(Ordering::SeqCst);
        let frontier_empty = {
            let f = frontier.lock().await;
            f.is_empty()
        };

        if active == 0 && frontier_empty {
            break;
        }

        tokio::select! {
            biased;

            res = &mut ctrl_c_stream, if !interrupted => {
                if res.is_ok() {
                    interrupted = true;
                    eprintln!("\n⚠️ Audit interrupted by user (Ctrl+C). Gracefully finalizing and saving crawled pages...");
                }
            }

            res = async {
                if let Some(ref mut rx) = cancel_rx {
                    rx.await.is_ok()
                } else {
                    std::future::pending::<bool>().await
                }
            }, if !interrupted && cancel_rx.is_some() => {
                if res {
                    interrupted = true;
                    eprintln!("\n⚠️ Audit interrupted by cancellation signal. Gracefully finalizing and saving crawled pages...");
                }
            }

            Some(outcome) = rx.recv() => {
                active_workers.fetch_sub(1, Ordering::SeqCst);

                for issue in &outcome.report.issues {
                    match issue.severity {
                        Severity::Critical => critical_count += 1,
                        Severity::Alert => alert_count += 1,
                        Severity::Warning => warning_count += 1,
                        Severity::Notice => {}
                    }
                    all_issues.push(issue.clone());
                }

                if let Some(ref writer) = db_writer {
                    let _ = writer.save_page(outcome.report.clone()).await;
                }

                let discovered_count = if config.max_depth == 0 || outcome.depth < config.max_depth {
                    let mut f = frontier.lock().await;

                    // Canonical facet pruning:
                    // If the current page is a parameterized/faceted URL whose canonical URL points
                    // to a base URL without those parameters (or canonical differs from current page),
                    // do not enqueue parameterized child links discovered on this page.
                    let is_canonicalized_away = match outcome.report.canonical_url {
                        Some(ref canon) => {
                            outcome.report.url.contains('?') && canon != &outcome.report.url
                        }
                        None => false,
                    };

                    for link in outcome.discovered_links {
                        if !link.is_internal || is_static_asset_url(&link.target_url) {
                            continue;
                        }

                        // Faceted defense 1: Prune sorting & display facets if configured
                        if config.ignore_sorting_facets && has_sorting_facets(&link.target_url) {
                            continue;
                        }

                        // Faceted defense 2: Prune excessive content query parameters
                        if config.max_query_params > 0
                            && count_content_facets(&link.target_url) > config.max_query_params
                        {
                            continue;
                        }

                        // Faceted defense 3: Canonical facet pruning (do not crawl deeper parameter variants from a non-canonical facet page)
                        if is_canonicalized_away && link.target_url.contains('?') {
                            continue;
                        }

                        let _ = f.push(&link.target_url, outcome.depth + 1, Some(&outcome.report.url));
                    }
                    f.enqueued_count() as usize
                } else {
                    let f = frontier.lock().await;
                    f.enqueued_count() as usize
                };

                if let Some(ref cb) = progress_cb {
                    let current_delay_ms = {
                        let a = aimd.lock().await;
                        a.current_delay_ms()
                    };

                    cb(ProgressUpdate {
                        crawled_pages: crawled_pages.len() + 1,
                        discovered_pages: discovered_count,
                        max_pages: config.max_pages,
                        current_url: outcome.report.url.clone(),
                        status_code: outcome.report.status_code,
                        ttfb_ms: outcome.report.ttfb_ms,
                        aimd_delay_ms: current_delay_ms,
                        critical_count,
                        alert_count,
                        warning_count,
                    });
                }

                crawled_pages.push(outcome.report);
            }
            else => {
                if active == 0 {
                    break;
                }
            }
        }
    }

    if interrupted {
        let drain_deadline = Instant::now() + Duration::from_secs(2);
        while active_workers.load(Ordering::SeqCst) > 0 && Instant::now() < drain_deadline {
            if let Ok(Some(outcome)) =
                tokio::time::timeout(Duration::from_millis(200), rx.recv()).await
            {
                active_workers.fetch_sub(1, Ordering::SeqCst);
                all_issues.extend(outcome.report.issues.clone());
                if let Some(ref writer) = db_writer {
                    let _ = writer.save_page(outcome.report.clone()).await;
                }
                crawled_pages.push(outcome.report);
            } else {
                break;
            }
        }
    }

    let final_aimd_delay = {
        let a = aimd.lock().await;
        a.current_delay_ms()
    };

    let (hit_frontier_page_limit, hit_frontier_depth_limit, frontier_has_remaining) = {
        let f = frontier.lock().await;
        (f.hit_max_pages(), f.hit_max_depth(), !f.is_empty())
    };

    let hit_max_pages = hit_max_pages
        || (config.max_pages > 0 && crawled_pages.len() >= config.max_pages as usize)
        || hit_frontier_page_limit;

    let is_partial_crawl =
        interrupted || hit_max_pages || hit_frontier_depth_limit || frontier_has_remaining;
    let crawl_exhaustive = !is_partial_crawl;

    let crawl_result = finalize_crawl(
        normalized_start,
        crawled_pages,
        all_issues,
        sitemap_urls,
        final_aimd_delay,
        start_time.elapsed(),
        crawl_exhaustive,
    );

    let final_status = if interrupted {
        "interrupted"
    } else {
        "completed"
    };
    if let Some(ref writer) = db_writer {
        let graph_issues: Vec<IssueFinding> = crawl_result
            .issues
            .iter()
            .filter(|i| i.category == crate::core::models::IssueCategory::SiteGraph)
            .cloned()
            .collect();
        if !graph_issues.is_empty() {
            let _ = writer.save_issues(graph_issues).await;
        }

        let criticals = crawl_result
            .issues
            .iter()
            .filter(|i| i.severity == Severity::Critical)
            .count() as u32;
        let alerts = crawl_result
            .issues
            .iter()
            .filter(|i| i.severity == Severity::Alert)
            .count() as u32;
        let warnings = crawl_result
            .issues
            .iter()
            .filter(|i| i.severity == Severity::Warning)
            .count() as u32;

        let _ = writer
            .update_status(
                final_status.to_string(),
                None,
                crawl_result.pages.len() as u32,
                criticals,
                alerts,
                warnings,
                Some(crawl_result.health_score),
            )
            .await;
        let _ = writer.flush().await;
    }

    Ok(crawl_result)
}
