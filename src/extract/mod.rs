//! # Site Extraction
//!
//! Reads a business website the way a brand analyst would and returns one JSON document:
//! the homepage and the few pages that matter (contact, about, catalog, locations), each as
//! Markdown with its links, contacts and metadata; the site's design (colours with usage and
//! evidence, fonts with the text they style, logo candidates); schema.org business facts; and
//! what it cost (time, fetches, bytes, renders).
//!
//! ## Pipeline
//!
//! 1. The homepage is fetched over plain HTTP through the [`guard`]ed fetcher.
//! 2. When it is an empty JavaScript shell or a bot challenge, it is rendered in headless
//!    Chromium ([`render`]); the browser starts lazily and only for pages that need it.
//! 3. Pages to read next are chosen by rules from the homepage's own links ([`select`]), fetched
//!    concurrently, and rendered one at a time when they need it.
//! 4. The homepage's stylesheets are fetched concurrently and analysed ([`css`]).
//!
//! Every step works within one overall deadline; a step that cannot finish in time is skipped
//! and the rest of the result is still returned.

pub mod contacts;
pub mod css;
pub mod fetch;
pub mod guard;
pub mod html;
pub mod proxy;
pub mod render;
pub mod select;

use crate::error::SeoResult;
use contacts::{find_emails, find_phones, find_social_links, find_whatsapp};
use css::{analyze_css, ColorEvidence, CssAnalysis, FontUse, DEFAULT_ICON_FONT_PATTERN};
use fetch::{Document, FetchFailure, FetcherOptions, GuardedFetcher};
use futures::stream::{self, StreamExt};
use guard::{parse_web_url, AddressPolicy};
use html::{analyze_page, LogoCandidate, LogoReason, PageAnalysis, PageLink, StructuredBusiness};
use regex::Regex;
use render::{RenderOptions, Renderer};
use schemars::transform::Transform;
use schemars::{JsonSchema, Schema};
use serde::Serialize;
use std::time::{Duration, Instant};
use url::Url;

/// Version of the JSON document; bumped on any breaking change.
pub const SCHEMA_VERSION: u32 = 1;
/// Most colours reported, most used first.
const MAX_COLORS: usize = 30;
/// Most font families reported.
const MAX_FONTS: usize = 30;
/// Most links reported per page.
const MAX_LINKS_PER_PAGE: usize = 200;
/// Pages and stylesheets fetched at the same time.
const FETCH_CONCURRENCY: usize = 4;
/// Least time left for retrying the homepage after a network error.
const HOMEPAGE_RETRY_RESERVE: Duration = Duration::from_secs(30);
/// Time kept back for stylesheets and output when deciding whether to render more pages.
const RENDER_RESERVE: Duration = Duration::from_secs(12);

/// When to render pages in Chromium.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderMode {
    /// Only pages that look like JavaScript shells or bot challenges.
    Auto,
    /// Every HTML page.
    Always,
    /// Never; plain HTTP only.
    Never,
}

/// Settings of one extraction.
#[derive(Debug, Clone)]
pub struct ExtractOptions {
    /// Pages read in total, homepage included.
    pub max_pages: usize,
    /// Most stylesheets fetched.
    pub max_stylesheets: usize,
    /// When to render.
    pub render: RenderMode,
    /// Browser settings for rendering.
    pub render_options: RenderOptions,
    /// User-Agent of plain fetches.
    pub user_agent: String,
    /// Allows private and loopback addresses (local test sites only).
    pub allow_private_network: bool,
    /// Egress proxy for all traffic; proxy environment variables are never used.
    pub proxy: Option<Url>,
    /// Longest the whole extraction may take.
    pub timeout: Duration,
    /// Longest one HTTP request may take.
    pub request_timeout: Duration,
    /// Largest HTML or CSS body read.
    pub max_body_bytes: usize,
    /// Pages to read after the homepage, chosen by the caller; selection by rules when empty.
    pub pages: Vec<String>,
}

impl ExtractOptions {
    /// Options with the defaults the CLI documents.
    pub fn new() -> Self {
        Self {
            max_pages: 6,
            max_stylesheets: 10,
            render: RenderMode::Auto,
            render_options: RenderOptions {
                page_timeout: Duration::from_secs(25),
                ..RenderOptions::default()
            },
            user_agent: crate::core::branding::EXTRACT_USER_AGENT.to_string(),
            allow_private_network: false,
            proxy: None,
            timeout: Duration::from_secs(90),
            request_timeout: Duration::from_secs(20),
            max_body_bytes: 2 * 1024 * 1024,
            pages: Vec::new(),
        }
    }
}

impl Default for ExtractOptions {
    fn default() -> Self {
        Self::new()
    }
}

/// How the extraction ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// The homepage was read.
    Ok,
    /// The URL is malformed or not http(s).
    UnsupportedUrl,
    /// The host name does not resolve.
    Unresolved,
    /// The host is a private or reserved address.
    BlockedAddress,
    /// The site could not be reached (connection, timeout, redirects).
    Unreachable,
    /// The homepage is a bot challenge even after rendering.
    BotProtected,
    /// The homepage answered with an HTTP error.
    HttpError,
    /// The homepage is not an HTML page.
    NotHtml,
}

/// One page read.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[schemars(transform = AlwaysPresent(&["title", "description", "lang"]))]
pub struct PageOut {
    /// The URL asked for.
    pub url: String,
    /// The URL after redirects.
    pub final_url: String,
    /// `home`, or the selection kind (`contact`, `about`, `catalog`, `locations`, `ordering`,
    /// `requested`).
    #[schemars(extend("enum" = ["home", "contact", "about", "catalog", "locations", "ordering", "requested"]))]
    pub kind: &'static str,
    /// HTTP status of the plain fetch.
    pub status: u16,
    /// Whether the content comes from a Chromium render.
    pub rendered: bool,
    /// Why it was rendered: `js_shell`, `bot_challenge`, `http_error` or `always`.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(extend("enum" = ["js_shell", "bot_challenge", "http_error", "always"]))]
    pub render_reason: Option<&'static str>,
    /// Bot-protection provider seen on the final content.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub waf: Option<&'static str>,
    /// `<title>`.
    #[schemars(with = "Option<String>")]
    pub title: Option<String>,
    /// Meta description.
    #[schemars(with = "Option<String>")]
    pub description: Option<String>,
    /// `<html lang>`.
    #[schemars(with = "Option<String>")]
    pub lang: Option<String>,
    /// Words of visible text.
    pub word_count: u32,
    /// The page as Markdown.
    pub markdown: String,
    /// The page's http(s) links.
    pub links: Vec<PageLink>,
    /// Phone numbers on the page.
    pub phones: Vec<String>,
    /// Email addresses on the page.
    pub emails: Vec<String>,
    /// WhatsApp chat links on the page.
    pub whatsapp: Vec<String>,
    /// Facebook page links (canonical).
    pub facebook: Vec<String>,
    /// Instagram profile links (canonical).
    pub instagram: Vec<String>,
}

/// A page that could not be read.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct PageError {
    /// The URL asked for.
    pub url: String,
    /// What went wrong.
    pub error: String,
}

/// A colour of the site with its use.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ColorOut {
    /// `#RRGGBB`.
    pub hex: String,
    /// Times it is written.
    pub occurrences: u32,
    /// Its 0-1 share of all colour uses.
    pub usage_score: f64,
    /// Where it was seen (up to 5).
    pub evidence: Vec<ColorEvidence>,
}

/// A stylesheet fetch.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[schemars(transform = AlwaysPresent(&["status"]))]
pub struct StylesheetOut {
    /// Where it was fetched from.
    pub url: String,
    /// HTTP status, when an answer came.
    #[schemars(with = "Option<u16>")]
    pub status: Option<u16>,
    /// Bytes read.
    pub bytes: usize,
    /// What went wrong, if anything.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The site's design as its homepage and stylesheets show it.
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
#[schemars(transform = AlwaysPresent(&["title", "description", "site_name", "theme_color"]))]
pub struct DesignOut {
    /// Homepage title.
    #[schemars(with = "Option<String>")]
    pub title: Option<String>,
    /// Homepage description.
    #[schemars(with = "Option<String>")]
    pub description: Option<String>,
    /// `og:site_name`.
    #[schemars(with = "Option<String>")]
    pub site_name: Option<String>,
    /// `theme-color` meta.
    #[schemars(with = "Option<String>")]
    pub theme_color: Option<String>,
    /// Colours, most used first.
    pub colors: Vec<ColorOut>,
    /// Font families, those with known roles first.
    pub fonts: Vec<FontUse>,
    /// Logo candidates, best first.
    pub logos: Vec<LogoCandidate>,
    /// Stylesheets read.
    pub stylesheets: Vec<StylesheetOut>,
}

/// Contacts merged across pages, in page order.
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct ContactsOut {
    /// Phone numbers as written.
    pub phones: Vec<String>,
    /// Email addresses, lowercased.
    pub emails: Vec<String>,
    /// WhatsApp chat links.
    pub whatsapp: Vec<String>,
    /// Canonical Facebook page links.
    pub facebook: Vec<String>,
    /// Canonical Instagram profile links.
    pub instagram: Vec<String>,
}

/// What the extraction cost.
#[derive(Debug, Clone, Default, Serialize, JsonSchema)]
pub struct StatsOut {
    /// Wall-clock milliseconds.
    pub elapsed_ms: u64,
    /// HTTP requests answered.
    pub http_fetches: u32,
    /// Body bytes read over HTTP.
    pub http_bytes: u64,
    /// Pages rendered in Chromium.
    pub rendered_pages: u32,
    /// Milliseconds spent rendering, browser start included.
    pub render_ms: u64,
    /// Browser requests refused (images, fonts, media, refused addresses).
    pub blocked_browser_requests: u32,
    /// Peak resident memory of this process in KiB (Linux only; Chromium not included).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peak_rss_kib: Option<u64>,
}

/// The extraction result.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[schemars(transform = AlwaysPresent(&["final_url", "design"]))]
pub struct SiteExtract {
    /// [`SCHEMA_VERSION`].
    pub schema_version: u32,
    /// `blacksparrow <version>`.
    pub generator: String,
    /// The URL asked for.
    pub start_url: String,
    /// The homepage after redirects.
    #[schemars(with = "Option<String>")]
    pub final_url: Option<String>,
    /// How it ended.
    pub outcome: Outcome,
    /// Details of a failed outcome.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome_detail: Option<String>,
    /// Pages read, homepage first.
    pub pages: Vec<PageOut>,
    /// Pages that could not be read.
    pub page_errors: Vec<PageError>,
    /// The site's design; `null` when the homepage could not be read.
    #[schemars(with = "Option<DesignOut>")]
    pub design: Option<DesignOut>,
    /// Contacts across pages.
    pub contacts: ContactsOut,
    /// schema.org business facts across pages.
    pub structured: StructuredBusiness,
    /// Cost of the run.
    pub stats: StatsOut,
}

/// Schema transform for nullable fields that are always serialized, as `null` when empty:
/// adds them to the object's `required` list without taking `null` out of their type.
pub(crate) struct AlwaysPresent(pub &'static [&'static str]);

impl Transform for AlwaysPresent {
    fn transform(&mut self, schema: &mut Schema) {
        let object = schema.ensure_object();
        let required = object
            .entry("required")
            .or_insert_with(|| serde_json::Value::Array(Vec::new()));
        if let Some(list) = required.as_array_mut() {
            list.extend(self.0.iter().map(|name| serde_json::Value::from(*name)));
        }
    }
}

/// JSON Schema (draft 2020-12) of the document `extract` prints, as `schema_version`
/// [`SCHEMA_VERSION`] defines it. The committed copy is `schema/extract.schema.json`.
pub fn output_schema() -> Schema {
    schemars::schema_for!(SiteExtract)
}

impl SiteExtract {
    fn failed(start_url: &str, outcome: Outcome, detail: Option<String>) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            generator: generator(),
            start_url: start_url.to_string(),
            final_url: None,
            outcome,
            outcome_detail: detail,
            pages: Vec::new(),
            page_errors: Vec::new(),
            design: None,
            contacts: ContactsOut::default(),
            structured: StructuredBusiness::default(),
            stats: StatsOut::default(),
        }
    }
}

fn generator() -> String {
    format!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
}

/// A page's content, from a plain fetch or a render.
struct Content {
    final_url: Url,
    status: u16,
    rendered: bool,
    render_reason: Option<&'static str>,
    waf: Option<&'static str>,
    analysis: PageAnalysis,
    cssom: Vec<String>,
}

/// Shared state of one run.
struct Run {
    options: ExtractOptions,
    fetcher: GuardedFetcher,
    renderer: Option<Renderer>,
    render_failed: bool,
    deadline: Instant,
    stats: StatsOut,
    started: Instant,
}

impl Run {
    fn remaining(&self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }

    fn count(&mut self, document: &Document) {
        self.stats.http_fetches += 1;
        self.stats.http_bytes += document.body.len() as u64;
    }

    /// Fetches a URL within the remaining time.
    async fn fetch(&mut self, url: &Url) -> Result<Document, FetchFailure> {
        let remaining = self.remaining();
        let document = tokio::time::timeout(remaining, self.fetcher.get_text(url))
            .await
            .unwrap_or_else(|_| Err(FetchFailure::Network("deadline".to_string())))?;
        self.count(&document);
        Ok(document)
    }

    /// Renders `url` when rendering is possible and time allows; `None` otherwise. `prefetched`
    /// is the page's HTML when the browser may reuse it instead of downloading it again.
    async fn render(&mut self, url: &Url, prefetched: Option<&str>) -> Option<render::Rendered> {
        if self.options.render == RenderMode::Never
            || self.render_failed
            || self.remaining() < RENDER_RESERVE
        {
            return None;
        }
        let started = Instant::now();
        if self.renderer.is_none() {
            let mut options = self.options.render_options.clone();
            options.policy = Some(self.fetcher.policy());
            options.upstream_proxy = self.options.proxy.clone();
            if options.user_agent.is_none() {
                options.user_agent = Some(self.options.user_agent.clone());
            }
            match Renderer::launch(options).await {
                Ok(renderer) => self.renderer = Some(renderer),
                Err(err) => {
                    tracing::warn!("rendering unavailable: {err}");
                    self.render_failed = true;
                    self.stats.render_ms += started.elapsed().as_millis() as u64;
                    return None;
                }
            }
        }
        let renderer = self.renderer.as_ref()?;
        let budget = self.remaining().saturating_sub(RENDER_RESERVE / 2);
        let result = tokio::time::timeout(budget, renderer.render_with(url, prefetched)).await;
        self.stats.render_ms += started.elapsed().as_millis() as u64;
        match result {
            Ok(Ok(rendered)) => {
                self.stats.rendered_pages += 1;
                self.stats.blocked_browser_requests += rendered.blocked_requests;
                Some(rendered)
            }
            Ok(Err(err)) => {
                tracing::info!("render failed for {url}: {err}");
                None
            }
            Err(_) => None,
        }
    }

    /// Turns a fetched document into page content, rendering it when it needs JavaScript.
    async fn content(&mut self, document: Document) -> Content {
        let analysis = analyze_page(&document.body, &document.final_url);
        let waf = challenge(document.waf, &analysis);
        let reason = match self.options.render {
            RenderMode::Never => None,
            RenderMode::Always => Some("always"),
            RenderMode::Auto => {
                if waf.is_some() {
                    Some("bot_challenge")
                } else if analysis.looks_like_js_shell() {
                    Some("js_shell")
                } else if matches!(document.status, 401 | 403 | 429 | 503) {
                    Some("http_error")
                } else {
                    None
                }
            }
        };
        let plain = Content {
            final_url: document.final_url.clone(),
            status: document.status,
            rendered: false,
            render_reason: None,
            waf,
            analysis,
            cssom: Vec::new(),
        };
        let Some(reason) = reason else {
            return plain;
        };
        // A shell page only needs its scripts run, so the browser reuses the downloaded HTML;
        // a challenge or an error page must be requested again for the server to decide anew.
        let prefetched =
            (reason == "js_shell" && !document.truncated).then_some(document.body.as_str());
        let Some(rendered) = self.render(&document.final_url, prefetched).await else {
            return plain;
        };
        let final_url = parse_web_url(&rendered.final_url).unwrap_or(document.final_url.clone());
        let analysis = analyze_page(&rendered.html, &final_url);
        let waf = challenge(
            crate::crawler::waf::detect_waf(200, &Default::default(), &rendered.html),
            &analysis,
        );
        // Keep the plain page when rendering produced less text and solved no challenge.
        if analysis.word_count < plain.analysis.word_count && waf.is_some() == plain.waf.is_some() {
            return plain;
        }
        Content {
            final_url,
            status: document.status,
            rendered: true,
            render_reason: Some(reason),
            waf,
            analysis,
            cssom: rendered.cssom,
        }
    }
}

/// Fetches `urls` with bounded concurrency, each within `remaining`, keeping their order.
async fn fetch_all(
    fetcher: GuardedFetcher,
    urls: Vec<String>,
    remaining: Duration,
) -> Vec<(String, Result<Document, FetchFailure>)> {
    stream::iter(urls)
        .map(|url| {
            let fetcher = fetcher.clone();
            async move {
                let result = tokio::time::timeout(remaining, fetcher.get_text_str(&url))
                    .await
                    .unwrap_or_else(|_| Err(FetchFailure::Network("deadline".into())));
                (url, result)
            }
        })
        .buffered(FETCH_CONCURRENCY)
        .collect()
        .await
}

/// The bot-protection provider when the page is its challenge screen. Protected sites also
/// embed the provider's scripts in their normal pages, so a page with real content is not a
/// challenge whatever markers it carries.
fn challenge(waf: Option<&'static str>, analysis: &PageAnalysis) -> Option<&'static str> {
    waf.filter(|_| analysis.word_count < 150)
}

/// Extracts a website. Never fails on the site's behalf: an unreachable or refused site is an
/// [`Outcome`] in the result.
///
/// # Errors
///
/// Returns an error only when the HTTP client cannot be built.
pub async fn extract_site(start_url: &str, options: ExtractOptions) -> SeoResult<SiteExtract> {
    let started = Instant::now();
    let Ok(start) = parse_web_url(start_url) else {
        return Ok(SiteExtract::failed(
            start_url,
            Outcome::UnsupportedUrl,
            Some("not an absolute http(s) URL".to_string()),
        ));
    };
    let fetcher = GuardedFetcher::new(FetcherOptions {
        user_agent: options.user_agent.clone(),
        timeout: options.request_timeout,
        max_body_bytes: options.max_body_bytes,
        policy: AddressPolicy::new(options.allow_private_network),
        proxy: options.proxy.clone(),
    })?;
    let icon_fonts = Regex::new(DEFAULT_ICON_FONT_PATTERN)
        .map_err(|e| crate::error::SeoError::Internal(e.to_string()))?;
    let mut run = Run {
        deadline: started + options.timeout,
        options,
        fetcher,
        renderer: None,
        render_failed: false,
        stats: StatsOut::default(),
        started,
    };

    let result = extract_with(&mut run, start_url, &start, &icon_fonts).await;
    if let Some(renderer) = run.renderer.take() {
        renderer.close().await;
    }
    let mut result = result;
    run.stats.elapsed_ms = run.started.elapsed().as_millis() as u64;
    run.stats.peak_rss_kib = peak_rss_kib();
    result.stats = run.stats;
    Ok(result)
}

async fn extract_with(
    run: &mut Run,
    start_url: &str,
    start: &Url,
    icon_fonts: &Regex,
) -> SiteExtract {
    // Homepage, with one retry: a dropped connection or a slow first answer is common on small
    // shared hosts and should not lose the whole site.
    let mut first = run.fetch(start).await;
    if matches!(first, Err(FetchFailure::Network(_))) && run.remaining() > HOMEPAGE_RETRY_RESERVE {
        tokio::time::sleep(Duration::from_secs(1)).await;
        first = run.fetch(start).await;
    }
    let document = match first {
        Ok(document) => document,
        Err(failure) => {
            let outcome = match &failure {
                FetchFailure::Refused(guard::GuardRefusal::Unresolved) => Outcome::Unresolved,
                FetchFailure::Refused(guard::GuardRefusal::BlockedAddress) => {
                    Outcome::BlockedAddress
                }
                FetchFailure::Refused(guard::GuardRefusal::Unsupported) => Outcome::UnsupportedUrl,
                _ => Outcome::Unreachable,
            };
            let detail = match failure {
                FetchFailure::Network(detail) => detail,
                other => other.label().to_string(),
            };
            return SiteExtract::failed(start_url, outcome, Some(detail));
        }
    };
    if !document.is_html() {
        let mut failed = SiteExtract::failed(
            start_url,
            Outcome::NotHtml,
            Some(document.content_type.clone()),
        );
        failed.final_url = Some(document.final_url.to_string());
        return failed;
    }
    let home = run.content(document).await;
    let outcome = if home.waf.is_some() {
        Outcome::BotProtected
    } else if home.status >= 400 && !home.rendered {
        Outcome::HttpError
    } else {
        Outcome::Ok
    };

    // Other pages: the caller's list, or the rules' choice from the homepage's links.
    let wanted: Vec<(String, &'static str)> = if run.options.pages.is_empty() {
        select::select_pages(
            &home.final_url,
            &home.analysis.links,
            run.options.max_pages.saturating_sub(1),
        )
        .into_iter()
        .map(|page| (page.url, page.kind))
        .collect()
    } else {
        run.options
            .pages
            .iter()
            .take(run.options.max_pages.saturating_sub(1))
            .map(|url| (url.clone(), "requested"))
            .collect()
    };
    // The homepage's stylesheets download while the other pages are read and rendered.
    let stylesheets: Vec<String> = home
        .analysis
        .stylesheet_urls
        .iter()
        .take(run.options.max_stylesheets)
        .cloned()
        .collect();
    let stylesheets = tokio::spawn(fetch_all(run.fetcher.clone(), stylesheets, run.remaining()));

    let mut page_errors = Vec::new();
    let mut others: Vec<(String, &'static str, Content)> = Vec::new();
    if outcome == Outcome::Ok && !wanted.is_empty() {
        let kinds: Vec<&'static str> = wanted.iter().map(|(_, kind)| *kind).collect();
        let urls: Vec<String> = wanted.into_iter().map(|(url, _)| url).collect();
        let fetched = fetch_all(run.fetcher.clone(), urls, run.remaining()).await;
        for ((url, result), kind) in fetched.into_iter().zip(kinds) {
            match result {
                Ok(document) if document.is_html() => {
                    run.count(&document);
                    let content = run.content(document).await;
                    if content.status >= 400 && !content.rendered {
                        page_errors.push(PageError {
                            url,
                            error: format!("HTTP {}", content.status),
                        });
                        continue;
                    }
                    others.push((url, kind, content));
                }
                Ok(document) => {
                    run.count(&document);
                    page_errors.push(PageError {
                        url,
                        error: format!("not html: {}", document.content_type),
                    })
                }
                Err(failure) => page_errors.push(PageError {
                    url,
                    error: failure.label().to_string(),
                }),
            }
        }
    }

    let stylesheets = stylesheets.await.unwrap_or_default();
    let design = design(run, &home, icon_fonts, stylesheets).await;
    let mut pages = vec![page_out(start_url.to_string(), "home", &home)];
    for (url, kind, content) in &others {
        pages.push(page_out(url.clone(), kind, content));
    }
    let mut structured = home.analysis.structured.clone();
    for (_, _, content) in &others {
        merge_structured(&mut structured, &content.analysis.structured);
    }
    let contacts = merge_contacts(&pages, &structured);
    SiteExtract {
        schema_version: SCHEMA_VERSION,
        generator: generator(),
        start_url: start_url.to_string(),
        final_url: Some(home.final_url.to_string()),
        outcome,
        outcome_detail: match outcome {
            Outcome::BotProtected => home.waf.map(str::to_string),
            Outcome::HttpError => Some(format!("HTTP {}", home.status)),
            _ => None,
        },
        pages,
        page_errors,
        design: Some(design),
        contacts,
        structured,
        stats: StatsOut::default(),
    }
}

fn page_out(url: String, kind: &'static str, content: &Content) -> PageOut {
    let analysis = &content.analysis;
    let markdown = &analysis.markdown;
    let mut facebook = Vec::new();
    let mut instagram = Vec::new();
    let link_text: String = analysis
        .links
        .iter()
        .map(|link| link.url.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    for text in [markdown.as_str(), link_text.as_str()] {
        let (fb, ig) = find_social_links(text);
        extend_unique(&mut facebook, fb);
        extend_unique(&mut instagram, ig);
    }
    let mut whatsapp = find_whatsapp(markdown);
    extend_unique(&mut whatsapp, find_whatsapp(&link_text));
    PageOut {
        url,
        final_url: content.final_url.to_string(),
        kind,
        status: content.status,
        rendered: content.rendered,
        render_reason: content.render_reason,
        waf: content.waf,
        title: analysis.title.clone(),
        description: analysis.description.clone(),
        lang: analysis.lang.clone(),
        word_count: analysis.word_count,
        markdown: markdown.clone(),
        links: analysis
            .links
            .iter()
            .take(MAX_LINKS_PER_PAGE)
            .cloned()
            .collect(),
        phones: find_phones(markdown),
        emails: find_emails(markdown),
        whatsapp,
        facebook,
        instagram,
    }
}

fn extend_unique(target: &mut Vec<String>, values: Vec<String>) {
    extend_unique_by(target, values, |value| value.to_string());
}

/// Appends the values whose `key` is not in `target` yet, keeping the first-seen spelling.
fn extend_unique_by(target: &mut Vec<String>, values: Vec<String>, key: impl Fn(&str) -> String) {
    for value in values {
        let wanted = key(&value);
        if !target.iter().any(|known| key(known) == wanted) && target.len() < contacts::MAX_CONTACTS
        {
            target.push(value);
        }
    }
}

fn merge_structured(target: &mut StructuredBusiness, other: &StructuredBusiness) {
    extend_unique(&mut target.names, other.names.clone());
    extend_unique(&mut target.telephones, other.telephones.clone());
    extend_unique(&mut target.emails, other.emails.clone());
    extend_unique(&mut target.addresses, other.addresses.clone());
    extend_unique(&mut target.same_as, other.same_as.clone());
    extend_unique(&mut target.logos, other.logos.clone());
    extend_unique(&mut target.descriptions, other.descriptions.clone());
}

fn merge_contacts(pages: &[PageOut], structured: &StructuredBusiness) -> ContactsOut {
    let phone = |value: &str| contacts::phone_key(value);
    let folded = |value: &str| value.to_lowercase();
    let mut contacts = ContactsOut::default();
    for page in pages {
        extend_unique_by(&mut contacts.phones, page.phones.clone(), phone);
        extend_unique_by(&mut contacts.emails, page.emails.clone(), folded);
        extend_unique_by(&mut contacts.whatsapp, page.whatsapp.clone(), folded);
        extend_unique_by(&mut contacts.facebook, page.facebook.clone(), folded);
        extend_unique_by(&mut contacts.instagram, page.instagram.clone(), folded);
    }
    extend_unique_by(&mut contacts.phones, structured.telephones.clone(), phone);
    extend_unique_by(&mut contacts.emails, structured.emails.clone(), folded);
    let profiles = structured.same_as.join(" ");
    let (facebook, instagram) = find_social_links(&profiles);
    extend_unique_by(&mut contacts.facebook, facebook, folded);
    extend_unique_by(&mut contacts.instagram, instagram, folded);
    contacts
}

/// Reads the homepage's design: inline styles, CSSOM rules, Google Fonts, theme colour and the
/// linked stylesheets (`fetched` already, plus one level of `@import`).
async fn design(
    run: &mut Run,
    home: &Content,
    icon_fonts: &Regex,
    fetched: Vec<(String, Result<Document, FetchFailure>)>,
) -> DesignOut {
    let analysis = &home.analysis;
    let page_url = home.final_url.to_string();
    let mut css = CssAnalysis::default();
    for (family, weights) in &analysis.google_fonts {
        if !icon_fonts.is_match(family) {
            css.add_weights(family, weights);
        }
    }
    for block in analysis.inline_styles.iter().chain(home.cssom.iter()) {
        analyze_css(
            &mut css,
            block,
            &page_url,
            true,
            icon_fonts,
            &analysis.class_roles,
        );
    }
    if let Some(theme) = &analysis.theme_color {
        css.add_color(
            theme,
            ColorEvidence {
                kind: "theme_color_meta",
                name: None,
                source_url: None,
            },
        );
    }

    let mut stylesheets: Vec<StylesheetOut> = Vec::new();
    let mut fetched = fetched;
    let mut level = 0;
    while !fetched.is_empty() {
        let mut imports: Vec<String> = Vec::new();
        for (url, result) in fetched {
            match result {
                Ok(document) => {
                    run.count(&document);
                    let ok = document.status < 400 && !document.content_type.contains("html");
                    if ok {
                        let before = css.imports.len();
                        analyze_css(
                            &mut css,
                            &document.body,
                            document.final_url.as_str(),
                            false,
                            icon_fonts,
                            &analysis.class_roles,
                        );
                        for import in css.imports[before..].iter() {
                            if let Ok(target) = document.final_url.join(import) {
                                let target = target.to_string();
                                let known = stylesheets.iter().any(|s| s.url == target)
                                    || imports.contains(&target);
                                if !known && css::google_fonts(&target).is_empty() {
                                    imports.push(target);
                                }
                            }
                        }
                    }
                    stylesheets.push(StylesheetOut {
                        url,
                        status: Some(document.status),
                        bytes: document.body.len(),
                        error: (!ok).then(|| "not a stylesheet".to_string()),
                    });
                }
                Err(failure) => stylesheets.push(StylesheetOut {
                    url,
                    status: None,
                    bytes: 0,
                    error: Some(failure.label().to_string()),
                }),
            }
        }
        let room = run
            .options
            .max_stylesheets
            .saturating_sub(stylesheets.len());
        level += 1;
        // One level of `@import` after the linked stylesheets, while time allows.
        fetched = if level < 2 && !imports.is_empty() && run.remaining() > Duration::from_secs(2) {
            let imports = imports.into_iter().take(room).collect();
            fetch_all(run.fetcher.clone(), imports, run.remaining()).await
        } else {
            Vec::new()
        };
    }

    let ranked = css.ranked_colors();
    let total: u32 = ranked.iter().map(|(_, count)| *count).sum();
    let colors = ranked
        .into_iter()
        .take(MAX_COLORS)
        .map(|(hex, occurrences)| ColorOut {
            evidence: css.evidence.get(&hex).cloned().unwrap_or_default(),
            usage_score: if total == 0 {
                0.0
            } else {
                ((occurrences as f64 / total as f64) * 10_000.0).round() / 10_000.0
            },
            hex,
            occurrences,
        })
        .collect();
    let mut fonts: Vec<FontUse> = css.fonts().to_vec();
    // Families that name the text they style are the brand's own choices.
    fonts.sort_by(|a, b| {
        a.selectors
            .is_empty()
            .cmp(&b.selectors.is_empty())
            .then_with(|| a.family.to_lowercase().cmp(&b.family.to_lowercase()))
    });
    fonts.truncate(MAX_FONTS);

    let mut logos = analysis.logo_candidates.clone();
    for url in &analysis.structured.logos {
        if !logos.iter().any(|logo| logo.url.as_deref() == Some(url)) {
            logos.push(LogoCandidate {
                reason: LogoReason::SchemaLogo,
                url: Some(url.clone()),
                svg: None,
            });
        }
    }
    logos.sort_by_key(|logo| logo.reason);

    DesignOut {
        title: analysis.title.clone(),
        description: analysis.description.clone(),
        site_name: analysis.site_name.clone(),
        theme_color: analysis.theme_color.clone(),
        colors,
        fonts,
        logos,
        stylesheets,
    }
}

/// Peak resident set size of this process in KiB, from `/proc/self/status` (Linux).
fn peak_rss_kib() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find(|line| line.starts_with("VmHWM:"))?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}
