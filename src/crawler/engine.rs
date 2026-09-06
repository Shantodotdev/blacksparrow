//! # Multi-Page Asynchronous Crawl Engine
//!
//! Coordinates concurrent fetching, frontier scheduling, robots.txt compliance,
//! streaming HTML parsing, AIMD politeness throttling, and post-crawl site graph analysis.

use crate::core::config::CrawlConfig;
use crate::core::models::{DiscoveredLink, IssueFinding, PageReport, Severity};
use crate::core::url::{is_internal, is_static_asset_url, normalize_url, url_hash};
use crate::crawler::aimd::AimdController;
use crate::crawler::client::{FetchOptions, FetchResult, HttpClient};
use crate::crawler::frontier::{Frontier, FrontierEntry};
use crate::crawler::robots::RobotsTxt;
use crate::crawler::sitemap::{parse_sitemap, SitemapDocument};
use crate::error::SeoResult;
use crate::graph::{compute_pagerank, SiteGraph};
use crate::parser::{parse_html, ParsedPage};
use crate::report::score::calculate_health_score;
use crate::rules::{evaluate_graph, evaluate_page};
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
    pub crawled_pages: usize,
    pub discovered_pages: usize,
    pub max_pages: u32,
    pub current_url: String,
    pub status_code: u16,
    pub ttfb_ms: u32,
    pub aimd_delay_ms: u64,
    pub critical_count: usize,
    pub alert_count: usize,
    pub warning_count: usize,
}

pub type ProgressCallback = Arc<dyn Fn(ProgressUpdate) + Send + Sync>;

#[derive(Debug, Clone)]
pub struct CrawlResult {
    pub target_url: String,
    pub pages: Vec<PageReport>,
    pub graph: SiteGraph,
    pub pagerank: HashMap<u64, f64>,
    pub issues: Vec<IssueFinding>,
    pub duration: Duration,
    pub sitemap_urls: Vec<String>,
    pub aimd_delay_ms: u64,
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
    url: &str,
    depth: u16,
    res: &FetchResult,
    parsed: Option<&ParsedPage>,
    issues: Vec<IssueFinding>,
) -> PageReport {
    let mut report = PageReport {
        crawl_id: CompactString::new("cli-session"),
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
    }

    report
}

async fn discover_robots_and_sitemaps(
    client: &HttpClient,
    seed_url: &str,
    respect_robots: bool,
) -> (Option<RobotsTxt>, Vec<String>) {
    let Ok(parsed_url) = url::Url::parse(seed_url) else {
        return (None, Vec::new());
    };

    let origin = format!("{}://{}", parsed_url.scheme(), parsed_url.authority());
    let robots_url = format!("{}/robots.txt", origin);
    let mut sitemap_feed_seeds = Vec::new();

    let robots = if respect_robots {
        if let Ok(res) = client.fetch(&robots_url).await {
            if res.status_code == 200 {
                let parsed_robots = RobotsTxt::parse(&res.body);
                for sm in parsed_robots.sitemaps() {
                    sitemap_feed_seeds.push(sm.to_string());
                }
                Some(parsed_robots)
            } else {
                None
            }
        } else {
            None
        }
    } else {
        None
    };

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
    (robots, sitemap_urls)
}

async fn fetch_and_audit_page(
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

            let report = build_page_report(url_str, depth, &res, parsed.as_ref(), page_issues);

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

    let graph_issues = evaluate_graph(&pages, &graph, &sitemap_urls, crawl_exhaustive);
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

pub async fn run_crawl(
    config: &CrawlConfig,
    progress_cb: Option<ProgressCallback>,
) -> SeoResult<CrawlResult> {
    let start_time = Instant::now();
    let normalized_start = normalize_url(&config.start_url)?;

    let client = Arc::new(HttpClient::new(FetchOptions {
        user_agent: config.user_agent.clone(),
        timeout: Duration::from_secs(30),
        max_redirects: 10,
        ..Default::default()
    })?);

    let (robots_txt, sitemap_urls) =
        discover_robots_and_sitemaps(&client, &normalized_start, config.respect_robots).await;

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

    loop {
        while let Ok(permit) = semaphore.clone().try_acquire_owned() {
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

                    tokio::spawn(async move {
                        let _permit = permit;
                        if let Some(outcome) = fetch_and_audit_page(
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

        let active = active_workers.load(Ordering::SeqCst);
        let frontier_empty = {
            let f = frontier.lock().await;
            f.is_empty()
        };

        if active == 0 && frontier_empty {
            break;
        }

        if config.max_pages > 0 && crawled_pages.len() >= config.max_pages as usize {
            hit_max_pages = true;
            break;
        }

        tokio::select! {
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

                let discovered_count = if config.max_depth == 0 || outcome.depth < config.max_depth {
                    let mut f = frontier.lock().await;
                    for link in outcome.discovered_links {
                        if link.is_internal && !is_static_asset_url(&link.target_url) {
                            let _ = f.push(&link.target_url, outcome.depth + 1, Some(&outcome.report.url));
                        }
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

    let final_aimd_delay = {
        let a = aimd.lock().await;
        a.current_delay_ms()
    };

    let crawl_exhaustive = !hit_max_pages;

    Ok(finalize_crawl(
        normalized_start,
        crawled_pages,
        all_issues,
        sitemap_urls,
        final_aimd_delay,
        start_time.elapsed(),
        crawl_exhaustive,
    ))
}
