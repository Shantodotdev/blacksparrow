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
use crate::core::models::{IssueFinding, PageReport, RuleId, Severity};
use crate::core::url::{
    contains_ignore_ascii_case, count_content_facets, has_sorting_facets, is_internal,
    is_static_asset_url, normalize_url, url_hash,
};
use crate::crawler::aimd::AimdController;
use crate::crawler::client::{FetchOptions, FetchResult, HttpClient};
use crate::crawler::frontier::{Frontier, FrontierEntry};
use crate::crawler::robots::RobotsTxt;
use crate::crawler::sitemap::{parse_sitemap, SitemapDocument};
use crate::error::{SeoError, SeoResult};
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

#[derive(Clone)]
struct WorkerConfig {
    no_aimd: bool,
    static_delay: u64,
    max_depth: u16,
    ignore_sorting_facets: bool,
    max_query_params: usize,
    include_re: Option<regex::Regex>,
    exclude_re: Option<regex::Regex>,
}

#[derive(Debug)]
struct WorkerPageOutcome {
    report: PageReport,
    candidate_urls: Vec<CompactString>,
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
    parsed: Option<ParsedPage>,
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
        // Allocation-free case-insensitive substring search over raw body bytes
        // avoids allocating full lowercased copies of HTML documents (up to 2.5GB across 50k pages).
        has_lorem_ipsum: contains_ignore_ascii_case(&res.body, "lorem ipsum"),
        is_https: url.starts_with("https://"),
        has_hsts: res.headers.contains_key("strict-transport-security"),
        has_csp: res.headers.contains_key("content-security-policy"),
        has_x_frame: res.headers.contains_key("x-frame-options"),
        has_x_content_type: res.headers.contains_key("x-content-type-options"),
        issues,
        ..Default::default()
    };

    // Zero-copy move semantics: transfer ownership of parsed structures (links, headings,
    // images, JSON-LD schemas, hreflangs) directly into the page report without cloning.
    if let Some(p) = parsed {
        report.title = p.title;
        report.title_length = report.title.as_ref().map(|t| t.len() as u16).unwrap_or(0);
        report.meta_description = p.meta_description;
        report.meta_desc_length = report
            .meta_description
            .as_ref()
            .map(|d| d.len() as u16)
            .unwrap_or(0);
        report.canonical_url = p.canonical_url;
        report.html_lang = p.html_lang;
        report.charset = p.charset;
        report.viewport = p.viewport;
        report.robots_flags = p.robots_flags;
        report.h1_primary = p.h1_primary;
        report.h1_count = p.h1_count;
        report.h2_headings = p.h2_headings;
        report.h3_headings = p.h3_headings;
        report.word_count = p.word_count;
        report.content_hash = p.content_hash;
        report.simhash = p.simhash;
        report.links = p.links;
        report.images = p.images;
        report.schemas = p.schemas;
        report.hreflangs = p.hreflangs;
        report.page_intent = p.page_intent;
    }

    report
}

/// Discovers and parses robots.txt and XML sitemaps before crawling starts.
///
/// 1. Probes `/robots.txt` and extracts sitemap declarations.
/// 2. Audits AI search crawler disallows (`GPTBot`, `ClaudeBot`, etc.) per Rule 11.1.
/// 3. Checks for presence of `/llms.txt` per Rule 11.2.
/// 4. If no sitemaps are declared in robots.txt, falls back to probing convention paths
///    (`/sitemap.xml`, `/sitemap_index.xml`, `/wp-sitemap.xml`) per docs/crawler.md §5.2.
/// 5. Recursively resolves nested sitemap index feeds up to 3 levels deep.
async fn discover_robots_and_sitemaps(
    client: &HttpClient,
    seed_url: &str,
    respect_robots: bool,
    explicit_sitemaps: &[String],
    max_pages: u32,
) -> (Option<RobotsTxt>, Vec<String>, Vec<IssueFinding>) {
    let Ok(parsed_url) = url::Url::parse(seed_url) else {
        return (None, Vec::new(), Vec::new());
    };

    let origin = format!("{}://{}", parsed_url.scheme(), parsed_url.authority());
    let robots_url = format!("{}/robots.txt", origin);
    let mut sitemap_feed_seeds = explicit_sitemaps.to_vec();
    let mut site_issues = Vec::new();

    let fetched_robots = match client.fetch(&robots_url).await {
        Ok(res) => {
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
            } else if res.status_code >= 500 && res.status_code < 600 {
                let rule = get_rule(RuleId::ErrHttp5xxServerError);
                let msg = format!(
                    "Robots.txt at '{}' returned HTTP {} server error. Under RFC 9309, crawlers must restrict or suspend crawling.",
                    robots_url, res.status_code
                );
                site_issues.push(rule.to_finding(&robots_url, Some(&msg)));
                Some(RobotsTxt::parse("User-agent: *\nDisallow: /\n"))
            } else {
                None
            }
        }
        Err(_) => {
            // If the host is completely unreachable at the network transport level,
            // avoid probing secondary feeds (llms.txt, fallback sitemaps) on a dead host.
            return (None, Vec::new(), site_issues);
        }
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

    // If robots.txt declared no sitemaps, probe standard conventions concurrently per docs/crawler.md §5.2
    if sitemap_feed_seeds.is_empty() {
        let u1 = format!("{}/sitemap.xml", origin);
        let u2 = format!("{}/sitemap_index.xml", origin);
        let u3 = format!("{}/wp-sitemap.xml", origin);

        let (r1, r2, r3) = tokio::join!(client.fetch(&u1), client.fetch(&u2), client.fetch(&u3),);

        for (candidate_url, res) in [(u1, r1), (u2, r2), (u3, r3)] {
            if let Ok(res) = res {
                if res.status_code == 200 {
                    sitemap_feed_seeds.push(candidate_url);
                }
            }
        }
    }

    // Recursively fetch and parse XML sitemaps up to 3 levels deep
    let mut queue = VecDeque::new();
    let mut visited_feeds = HashSet::new();
    let mut discovered_pages_set = HashSet::new();
    let mut discovered_pages = Vec::new();

    for feed in sitemap_feed_seeds {
        queue.push_back((feed, 0u8));
    }

    let max_sitemap_target = if max_pages > 0 {
        (max_pages as usize).saturating_mul(3).max(10_000)
    } else {
        usize::MAX
    };

    while let Some((feed_url, depth)) = queue.pop_front() {
        if discovered_pages.len() >= max_sitemap_target {
            break;
        }
        if depth > 3 || !visited_feeds.insert(feed_url.clone()) {
            continue;
        }

        if let Ok(res) = client.fetch(&feed_url).await {
            if res.status_code == 200 {
                if let Ok(doc) = parse_sitemap(&res.body_bytes) {
                    match doc {
                        SitemapDocument::UrlSet(entries) => {
                            for entry in entries {
                                if discovered_pages.len() >= max_sitemap_target {
                                    break;
                                }
                                let loc = entry.loc.as_str();
                                if is_internal(loc, &origin)
                                    && !is_static_asset_url(loc)
                                    && !loc.ends_with(".xml")
                                    && !loc.ends_with(".xml.gz")
                                {
                                    let candidate =
                                        normalize_url(loc).unwrap_or_else(|_| loc.to_string());
                                    if discovered_pages_set.insert(candidate.clone()) {
                                        discovered_pages.push(candidate);
                                    }
                                }
                            }
                        }
                        SitemapDocument::Index(sub_sitemaps) => {
                            if depth < 3 && discovered_pages.len() < max_sitemap_target {
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

    (robots, discovered_pages, site_issues)
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
    worker_cfg: Arc<WorkerConfig>,
) -> Option<WorkerPageOutcome> {
    if !worker_cfg.no_aimd && worker_cfg.static_delay == 0 {
        let delay_ms = {
            let a = aimd.lock().await;
            a.current_delay_ms()
        };
        if delay_ms > 0 {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
        }
    } else if worker_cfg.static_delay > 0 {
        tokio::time::sleep(Duration::from_millis(worker_cfg.static_delay)).await;
    }

    let url_str = entry.url.as_str();
    let fetch_res = client.fetch(url_str).await;

    match fetch_res {
        Ok(res) => {
            if !worker_cfg.no_aimd && worker_cfg.static_delay == 0 {
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

            let depth = entry.depth;

            // Parallel link filtering, normalization, and deduplication across worker green tasks:
            // Performing URL parsing, regex evaluations, query facet pruning, and normalization
            // inside concurrent Tokio worker tasks distributes heavy CPU work across all cores,
            // preventing the single coordinator task from locking the frontier mutex for long durations.
            let candidate_urls = if let Some(ref p) = parsed {
                if worker_cfg.max_depth == 0 || depth < worker_cfg.max_depth {
                    let is_canonicalized_away = match p.canonical_url {
                        Some(ref canon) => url_str.contains('?') && canon != url_str,
                        None => false,
                    };

                    let mut candidates = Vec::with_capacity(p.links.len());
                    // Deduplicate candidate links locally per page to reduce channel traffic and frontier heap operations
                    let mut local_seen = HashSet::with_capacity(p.links.len());

                    for link in &p.links {
                        if !link.is_internal || is_static_asset_url(&link.target_url) {
                            continue;
                        }

                        // Faceted defense 1: Prune sorting & display facets if configured
                        if worker_cfg.ignore_sorting_facets && has_sorting_facets(&link.target_url)
                        {
                            continue;
                        }

                        // Faceted defense 2: Prune excessive content query parameters
                        if worker_cfg.max_query_params > 0
                            && count_content_facets(&link.target_url) > worker_cfg.max_query_params
                        {
                            continue;
                        }

                        // Faceted defense 3: Canonical facet pruning
                        if is_canonicalized_away && link.target_url.contains('?') {
                            continue;
                        }

                        // Path filter: Include regex
                        if let Some(ref inc) = worker_cfg.include_re {
                            if !inc.is_match(&link.target_url) {
                                continue;
                            }
                        }

                        // Path filter: Exclude regex
                        if let Some(ref exc) = worker_cfg.exclude_re {
                            if exc.is_match(&link.target_url) {
                                continue;
                            }
                        }

                        if let Ok(normalized) = normalize_url(&link.target_url) {
                            let compact = CompactString::new(&normalized);
                            if local_seen.insert(compact.clone()) {
                                candidates.push(compact);
                            }
                        }
                    }
                    candidates
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            };

            let report = build_page_report(session_id, url_str, depth, &res, parsed, page_issues);

            Some(WorkerPageOutcome {
                report,
                candidate_urls,
                depth,
            })
        }
        Err(_) => {
            if !worker_cfg.no_aimd && worker_cfg.static_delay == 0 {
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
                .as_micros()
        )
    });

    let client = Arc::new(HttpClient::new(FetchOptions {
        user_agent: config.user_agent.clone(),
        timeout: Duration::from_secs(30),
        max_redirects: 10,
        custom_headers: config.headers.clone(),
        proxy: config.proxy.clone(),
        ..Default::default()
    })?);

    let (robots_txt, sitemap_urls, site_issues) = discover_robots_and_sitemaps(
        &client,
        &normalized_start,
        config.respect_robots,
        &config.explicit_sitemaps,
        config.max_pages,
    )
    .await;

    if let Some(ref writer) = db_writer {
        let _ = writer.save_issues(site_issues.clone()).await;
    }

    let include_re = match config.include_regex.as_deref() {
        Some(pat) => Some(regex::Regex::new(pat).map_err(|e| {
            SeoError::Config(format!("Invalid include regex pattern '{pat}': {e}"))
        })?),
        None => None,
    };
    let exclude_re = match config.exclude_regex.as_deref() {
        Some(pat) => Some(regex::Regex::new(pat).map_err(|e| {
            SeoError::Config(format!("Invalid exclude regex pattern '{pat}': {e}"))
        })?),
        None => None,
    };

    let worker_config = Arc::new(WorkerConfig {
        no_aimd: config.no_aimd,
        static_delay: config.delay_ms,
        max_depth: config.max_depth,
        ignore_sorting_facets: config.ignore_sorting_facets,
        max_query_params: config.max_query_params,
        include_re: include_re.clone(),
        exclude_re: exclude_re.clone(),
    });

    let frontier = Arc::new(Mutex::new(Frontier::new(
        config.max_pages,
        config.max_depth,
    )));

    {
        let mut f = frontier.lock().await;
        let filtered_sitemaps: Vec<String> = sitemap_urls
            .iter()
            .filter(|u| {
                if let Some(ref inc) = include_re {
                    if !inc.is_match(u) {
                        return false;
                    }
                }
                if let Some(ref exc) = exclude_re {
                    if exc.is_match(u) {
                        return false;
                    }
                }
                true
            })
            .cloned()
            .collect();
        f.register_sitemap_urls(&filtered_sitemaps);
        f.push(&normalized_start, 0, None)?;

        // Seed the frontier with discovered XML sitemap URLs so they are crawled
        for sm_url in &filtered_sitemaps {
            if sm_url == &normalized_start {
                continue;
            }
            if config.max_pages > 0 && f.enqueued_count() >= config.max_pages {
                break;
            }
            let _ = f.push(sm_url, 1, None);
        }
    }

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

    if let Some(ref cb) = progress_cb {
        let f = frontier.lock().await;
        cb(ProgressUpdate {
            crawled_pages: 0,
            discovered_pages: f.enqueued_count() as usize,
            max_pages: config.max_pages,
            current_url: normalized_start.clone(),
            status_code: 0,
            ttfb_ms: 0,
            aimd_delay_ms: config.delay_ms,
            critical_count,
            alert_count,
            warning_count,
        });
    }

    let aimd = Arc::new(Mutex::new(AimdController::new(
        config.concurrency,
        config.delay_ms,
    )));

    let concurrency = config.concurrency.max(1);
    let semaphore = Arc::new(Semaphore::new(concurrency));
    let active_workers = Arc::new(AtomicUsize::new(0));

    let (tx, mut rx) = mpsc::channel::<Option<WorkerPageOutcome>>(concurrency * 2);

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
                    let worker_cfg_clone = Arc::clone(&worker_config);
                    let tx_clone = tx.clone();
                    let sess_id = session_id.clone();

                    tokio::spawn(async move {
                        let _permit = permit;
                        let outcome = fetch_and_audit_page(
                            &sess_id,
                            entry,
                            client_clone,
                            aimd_clone,
                            worker_cfg_clone,
                        )
                        .await;
                        let _ = tx_clone.send(outcome).await;
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

            Some(opt) = rx.recv() => {
                active_workers.fetch_sub(1, Ordering::SeqCst);

                if let Some(outcome) = opt {
                    for issue in &outcome.report.issues {
                        match issue.severity {
                            Severity::Critical => critical_count += 1,
                            Severity::Alert => alert_count += 1,
                            Severity::Warning => warning_count += 1,
                            Severity::Notice => {}
                        }
                    }

                    if let Some(ref writer) = db_writer {
                        let _ = writer.save_page(outcome.report.clone()).await;
                    }

                    // Single-lock batch push: enqueues all pre-normalized candidate URLs under a single
                    // lock acquisition, minimizing contention with worker tasks calling frontier.pop().
                    let discovered_count = {
                        let mut f = frontier.lock().await;
                        if !outcome.candidate_urls.is_empty() {
                            f.push_normalized_batch(
                                &outcome.candidate_urls,
                                outcome.depth + 1,
                                Some(&outcome.report.url),
                            );
                        }
                        f.enqueued_count() as usize
                    };

                    if let Some(ref cb) = progress_cb {
                        // Avoid locking the AIMD controller mutex if AIMD politeness throttling is disabled
                        let current_delay_ms = if config.no_aimd {
                            0
                        } else if config.delay_ms > 0 {
                            config.delay_ms
                        } else {
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
            if let Ok(Some(opt)) = tokio::time::timeout(Duration::from_millis(200), rx.recv()).await
            {
                active_workers.fetch_sub(1, Ordering::SeqCst);
                if let Some(outcome) = opt {
                    if let Some(ref writer) = db_writer {
                        let _ = writer.save_page(outcome.report.clone()).await;
                    }
                    crawled_pages.push(outcome.report);
                }
            } else {
                break;
            }
        }
    }

    let final_aimd_delay = if config.no_aimd {
        0
    } else if config.delay_ms > 0 {
        config.delay_ms
    } else {
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

    // Batch aggregate all page issues once at crawl termination, eliminating tens of thousands
    // of redundant issue clone operations from inside the high-frequency coordinator loop.
    for page in &crawled_pages {
        all_issues.extend(page.issues.iter().cloned());
    }

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
    } else if crawl_result.pages.is_empty() {
        "failed"
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
