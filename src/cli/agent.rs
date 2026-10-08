//! # Agent-Mode Command Handlers
//!
//! `scrape`, `map`, `crawl`, `find`, `extract`, `interact` and (feature `serve`) `serve`:
//! web pages to clean Markdown and structured data for AI agents. Results go to stdout
//! (Markdown, one URL per line, NDJSON or JSON); progress and warnings go to stderr.

use crate::cli::args::{
    CrawlArgs, ExtractArgs, FindArgs, InteractArgs, MapArgs, PageArgs, ScrapeArgs, WebArgs,
};
use crate::extract::crawl::{crawl_site, CrawlControl, CrawlOptions};
use crate::extract::fields::{extract, ExtractRequest};
use crate::extract::find::{find_in_crawl, find_on_page, FindOptions, FindRequest, FindResult};
use crate::extract::interact::{interact, InteractRequest};
use crate::extract::map::{map_site, MapOptions, SitemapMode};
use crate::extract::scrape::{Scraper, ScraperConfig};
use crate::extract::sink::{DirSink, NdjsonSink, PageSink};
use crate::extract::types::{
    BrowserAction, OutputFormat, PageDocument, PageStatus, RenderMode, ScrapeOptions,
};
use crate::storage::resolve_db_path;
use base64::Engine;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use std::error::Error;
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

type CliResult = Result<(), Box<dyn Error>>;

/// Scraper settings from the shared network and storage flags.
pub fn scraper_config(web: &WebArgs) -> Result<ScraperConfig, Box<dyn Error>> {
    let mut headers = Vec::new();
    for raw in &web.headers {
        let (name, value) = raw
            .split_once(':')
            .ok_or_else(|| format!("Header '{raw}' must look like 'Name: value'"))?;
        headers.push((name.trim().to_string(), value.trim().to_string()));
    }
    let defaults = ScraperConfig::default();
    Ok(ScraperConfig {
        user_agent: web.user_agent.clone().unwrap_or(defaults.user_agent),
        timeout: Duration::from_secs(web.timeout.max(1)),
        headers,
        allow_all_private_ips: web.allow_local_network,
        allowed_private_hosts: web.allowed_hosts.clone(),
        respect_robots: !web.no_robots,
        chrome_ws: web.chrome_ws.clone(),
        render_concurrency: web.render_concurrency.max(1),
        db_path: (!web.no_store).then(|| resolve_db_path(web.db_path.clone(), web.local)),
        ..defaults
    })
}

fn parse_formats(list: &str) -> Result<Vec<OutputFormat>, Box<dyn Error>> {
    let mut formats = Vec::new();
    for name in list.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let format: OutputFormat = serde_json::from_value(json!(name.replace('-', "_")))
            .map_err(|_| {
                format!(
                    "Unknown format '{name}'. Use markdown, json, text, links, metadata, html, raw_html or screenshot"
                )
            })?;
        if !formats.contains(&format) {
            formats.push(format);
        }
    }
    if formats.is_empty() {
        formats.push(OutputFormat::Markdown);
    }
    Ok(formats)
}

fn parse_named<T: DeserializeOwned>(value: &str, what: &str) -> Result<T, Box<dyn Error>> {
    serde_json::from_value(json!(value.trim().to_ascii_lowercase()))
        .map_err(|_| format!("Unknown {what} '{value}'").into())
}

/// Scrape options from the page flags.
pub fn scrape_options(page: &PageArgs) -> Result<ScrapeOptions, Box<dyn Error>> {
    let render: RenderMode = parse_named(&page.render, "render mode")?;
    Ok(ScrapeOptions {
        formats: parse_formats(&page.format)?,
        only_main_content: !page.full_page,
        include_selectors: page.include_selectors.clone(),
        exclude_selectors: page.exclude_selectors.clone(),
        max_tokens: page.max_tokens,
        render,
        wait_for_selector: page.wait_for.clone(),
        max_age_secs: page.max_age,
        ..Default::default()
    })
}

/// Reads JSON from a file path, or parses the argument itself as JSON.
fn json_arg(value: &str) -> Result<Value, Box<dyn Error>> {
    let path = std::path::Path::new(value);
    let text = if !value.trim_start().starts_with(['{', '[']) && path.exists() {
        std::fs::read_to_string(path)?
    } else {
        value.to_string()
    };
    serde_json::from_str(&text).map_err(|e| format!("Invalid JSON in '{value}': {e}").into())
}

fn print_json<T: serde::Serialize>(value: &T) -> CliResult {
    let mut out = std::io::stdout().lock();
    serde_json::to_writer_pretty(&mut out, value)?;
    writeln!(out)?;
    Ok(())
}

/// A document that was never fetched (refused URL, network failure) is an error.
fn fail_unfetched(doc: &PageDocument) {
    if doc.status_code == 0 && doc.status != PageStatus::Ok {
        eprintln!(
            "❌ {}: {}",
            doc.url,
            doc.error.as_deref().unwrap_or(doc.status.as_str())
        );
        std::process::exit(2);
    }
}

/// `scrape`: one page to Markdown (or JSON).
pub async fn handle_scrape(args: ScrapeArgs) -> CliResult {
    let scraper = Scraper::new(scraper_config(&args.web)?)?;
    let mut opts = scrape_options(&args.page)?;
    let markdown_only = !args.json && opts.formats == [OutputFormat::Markdown];
    if args.json && !opts.wants(OutputFormat::Metadata) {
        opts.formats.push(OutputFormat::Metadata);
    }
    let doc = scraper.scrape(&args.url, &opts).await?;
    fail_unfetched(&doc);

    let rendered = if markdown_only {
        doc.markdown.clone()
    } else {
        serde_json::to_string_pretty(&doc)?
    };
    match &args.output {
        Some(path) => std::fs::write(path, rendered)?,
        None => println!("{}", rendered.trim_end()),
    }
    if doc.status != PageStatus::Ok {
        eprintln!(
            "⚠️  {} returned {} (HTTP {}){}",
            doc.url,
            doc.status.as_str(),
            doc.status_code,
            doc.error
                .as_deref()
                .map(|e| format!(": {e}"))
                .unwrap_or_default()
        );
        std::process::exit(1);
    }
    Ok(())
}

/// `map`: a site's URLs, one per line (or JSON).
pub async fn handle_map(args: MapArgs) -> CliResult {
    let scraper = Scraper::new(scraper_config(&args.web)?)?;
    let sitemap: SitemapMode = parse_named(&args.sitemap, "sitemap mode")?;
    let opts = MapOptions {
        search: args.search.clone(),
        include_paths: args.include_paths.clone(),
        exclude_paths: args.exclude_paths.clone(),
        limit: args.limit,
        sitemap,
        include_subdomains: args.subdomains,
        ..Default::default()
    };
    let result = map_site(&scraper, &args.url, &opts).await?;
    if args.json {
        return print_json(&result);
    }
    let mut out = std::io::stdout().lock();
    for link in &result.links {
        writeln!(out, "{}", link.url)?;
    }
    eprintln!(
        "{} URLs ({} from sitemaps, {} disallowed by robots.txt)",
        result.links.len(),
        result.sitemap_urls,
        result.robots_skipped
    );
    Ok(())
}

/// `crawl`: many pages to Markdown files (`--out`) or NDJSON on stdout.
pub async fn handle_crawl(args: CrawlArgs) -> CliResult {
    let scraper = Scraper::new(scraper_config(&args.web)?)?;
    let sitemap: SitemapMode = parse_named(&args.sitemap, "sitemap mode")?;
    let opts = CrawlOptions {
        limit: args.limit,
        max_depth: args.max_depth,
        include_paths: args.include_paths.clone(),
        exclude_paths: args.exclude_paths.clone(),
        sitemap,
        allow_subdomains: args.subdomains,
        concurrency: args.concurrency.max(1),
        delay_ms: args.delay,
        dedupe_boilerplate: !args.keep_boilerplate,
        scrape: scrape_options(&args.page)?,
        ..Default::default()
    };

    let control = Arc::new(CrawlControl::default());
    let on_interrupt = control.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            eprintln!("Stopping after the pages in flight…");
            on_interrupt.cancel();
        }
    });

    let mut dir_sink;
    let mut ndjson_sink;
    let sink: &mut dyn PageSink = match &args.out {
        Some(dir) => {
            dir_sink = DirSink::new(dir)?;
            &mut dir_sink
        }
        None => {
            ndjson_sink = NdjsonSink::new(std::io::BufWriter::new(std::io::stdout()));
            &mut ndjson_sink
        }
    };
    crawl_site(&scraper, &args.url, &opts, sink, &control).await?;

    let progress = control.progress();
    let destination = args
        .out
        .as_ref()
        .map(|d| format!(" → {}", d.display()))
        .unwrap_or_default();
    eprintln!(
        "{} pages crawled, {} failed, {} disallowed by robots.txt ({}){destination}",
        progress.pages_done,
        progress.pages_failed,
        progress.pages_skipped,
        progress.state.as_str()
    );
    if let Some(error) = progress.error {
        return Err(error.into());
    }
    Ok(())
}

/// `find`: passages on a page or across a stored crawl, as JSON.
pub async fn handle_find(args: FindArgs) -> CliResult {
    let scraper = Scraper::new(scraper_config(&args.web)?)?;
    let req = FindRequest {
        url: args.url.clone(),
        crawl_id: args.crawl.clone(),
        url_prefix: args.url_prefix.clone(),
        query: args.query.clone(),
        selector: args.selector.clone(),
        regex: args.regex.clone(),
        options: FindOptions {
            top_k: args.top_k,
            attributes: args.attributes.clone(),
            ..Default::default()
        },
        ..Default::default()
    };
    let result = if req.url.is_some() {
        find_on_page(&scraper, &req).await?
    } else if req.crawl_id.is_some() || req.url_prefix.is_some() {
        let db = scraper
            .database()
            .ok_or("Searching stored pages needs storage (remove --no-store)")?;
        FindResult {
            mode: if req.regex.is_some() {
                "regex"
            } else {
                "query"
            }
            .to_string(),
            hits: find_in_crawl(&db.connect()?, &req)?,
            ..Default::default()
        }
    } else {
        return Err("Give a URL, or --crawl <ID> / --url-prefix to search stored pages".into());
    };
    if let Some(error) = &result.error {
        eprintln!("⚠️  {error}");
    }
    print_json(&result)
}

/// `extract`: schema-shaped JSON from pages.
pub async fn handle_extract(args: ExtractArgs) -> CliResult {
    let scraper = Scraper::new(scraper_config(&args.web)?)?;
    let rules = match &args.rules {
        Some(r) => Some(serde_json::from_value(json_arg(r)?)?),
        None => None,
    };
    let req = ExtractRequest {
        urls: args.urls.clone(),
        crawl_id: args.crawl.clone(),
        schema: json_arg(&args.schema)?,
        rules,
        learn: !args.no_learn,
        min_confidence: args.min_confidence,
        limit: args.limit,
        ..Default::default()
    };
    let results = extract(&scraper, &req).await?;
    for r in &results {
        if let Some(error) = &r.error {
            eprintln!("⚠️  {}: {error}", r.url);
        }
    }
    let data = match results.as_slice() {
        [one] => one.data.clone(),
        many => Value::Array(many.iter().map(|r| r.data.clone()).collect()),
    };
    print_json(&json!({ "data": data, "results": results }))
}

/// `interact`: browser steps, then the page as Markdown and its interactive elements.
pub async fn handle_interact(args: InteractArgs) -> CliResult {
    let scraper = Scraper::new(scraper_config(&args.web)?)?;
    let actions: Vec<BrowserAction> = match &args.steps {
        Some(steps) => serde_json::from_value(json_arg(steps)?)?,
        None => Vec::new(),
    };
    let req = InteractRequest {
        url: args.url.clone(),
        actions,
        screenshot: args.screenshot.is_some(),
        wait_for: args.wait_for.clone(),
        ..Default::default()
    };
    let mut result = interact(&scraper, &req).await?;
    if let (Some(path), Some(png)) = (&args.screenshot, result.document.screenshot.take()) {
        std::fs::write(path, base64::engine::general_purpose::STANDARD.decode(png)?)?;
        eprintln!("Screenshot saved to {}", path.display());
    }
    for error in &result.action_errors {
        eprintln!("⚠️  {error}");
    }
    if args.json {
        return print_json(&result);
    }
    let mut out = std::io::stdout().lock();
    writeln!(out, "{}", result.document.markdown.trim_end())?;
    if !result.snapshot.is_empty() {
        writeln!(out, "\n---\nInteractive elements:")?;
        for element in &result.snapshot {
            writeln!(out, "{element}")?;
        }
    }
    Ok(())
}

/// `serve`: the Firecrawl-compatible HTTP API.
#[cfg(feature = "serve")]
pub async fn handle_serve(args: crate::cli::args::ServeArgs) -> CliResult {
    use crate::serve::{serve, AppState, ServeConfig};

    let mut api_keys = args.api_keys.clone();
    if let Ok(env_keys) = std::env::var("BLACKSPARROW_API_KEYS") {
        api_keys.extend(
            env_keys
                .split(',')
                .map(str::trim)
                .filter(|k| !k.is_empty())
                .map(str::to_string),
        );
    }
    let host: std::net::IpAddr = args
        .host
        .parse()
        .map_err(|_| format!("Invalid --host '{}': use an IP address", args.host))?;
    let addr = std::net::SocketAddr::new(host, args.port);
    let scraper = Scraper::new(scraper_config(&args.web)?)?;
    let config = ServeConfig {
        api_keys,
        requests_per_minute: args.rate_limit,
        max_body_bytes: args.max_body_bytes,
        max_crawl_pages: args.max_crawl_pages,
        max_concurrent_crawls: args.max_concurrent_crawls,
        public_url: args.public_url.clone(),
    };
    eprintln!("blacksparrow API on http://{addr} (Ctrl-C to stop)");
    serve(addr, AppState::new(scraper, config)).await?;
    Ok(())
}
