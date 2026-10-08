//! HTTP API for agent-mode crawling (`blacksparrow serve`, feature `serve`).
//!
//! Endpoints follow Firecrawl's request and response shapes so existing clients and SDKs
//! can point at a self-hosted server:
//!
//! | Method | Path | Purpose |
//! |---|---|---|
//! | `POST` | `/v1/scrape`, `/v2/scrape` | One page to Markdown and metadata |
//! | `POST` | `/v1/map` (URL strings), `/v2/map` (objects) | List a site's URLs |
//! | `POST` | `/v1/crawl`, `/v2/crawl` | Start a background crawl |
//! | `GET` / `DELETE` | `/v1/crawl/{id}` | Poll (paged with `skip`/`limit`) or cancel |
//! | `POST` | `/v1/find` | Passages by question, selector or regex |
//! | `POST` | `/v1/extract` | Schema-shaped data without an LLM (synchronous) |
//! | `POST` | `/v1/interact` | Browser steps, then read the page |
//! | `GET` | `/health` | Liveness (no key needed) |
//!
//! Every other route needs `Authorization: Bearer <key>` when keys are configured. Without
//! keys the server refuses to listen beyond loopback ([`check_bind`]). Each key has a
//! per-minute request budget; crawls are capped in pages and in how many run at once.
//! Private and link-local addresses are refused unless the scraper was configured to allow
//! them, and cloud metadata endpoints are always refused.

mod firecrawl;

use crate::core::url::validate_url_safety;
use crate::error::{SeoError, SeoResult};
use crate::extract::crawl::{CrawlOptions, CrawlState};
use crate::extract::fields::{extract as extract_fields, ExtractRequest};
use crate::extract::find::{find_in_crawl, find_on_page, FindRequest, FindResult};
use crate::extract::interact::{interact as interact_page, InteractRequest};
use crate::extract::jobs::CrawlJobs;
use crate::extract::map::{map_site, MapOptions};
use crate::extract::scrape::Scraper;
use crate::extract::types::{OutputFormat, PageStatus, ScrapeOptions};
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Path, Query, Request, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use firecrawl::{
    convert_actions, document_json, normalize_nested, normalize_scrape, normalize_sitemap,
};
use hashbrown::HashMap;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Server limits and credentials.
#[derive(Debug, Clone)]
pub struct ServeConfig {
    /// Accepted bearer keys. Empty means no authentication (loopback only).
    pub api_keys: Vec<String>,
    /// Requests per key per minute (`0` = unlimited).
    pub requests_per_minute: u32,
    /// Largest accepted request body in bytes.
    pub max_body_bytes: usize,
    /// Most pages one crawl or extract request may process.
    pub max_crawl_pages: usize,
    /// Most crawls running at once.
    pub max_concurrent_crawls: usize,
    /// External base URL used in crawl status links (defaults to the request's host).
    pub public_url: Option<String>,
}

impl Default for ServeConfig {
    fn default() -> Self {
        Self {
            api_keys: Vec::new(),
            requests_per_minute: 120,
            max_body_bytes: 1024 * 1024,
            max_crawl_pages: 1000,
            max_concurrent_crawls: 4,
            public_url: None,
        }
    }
}

/// Shared server state: one scraper, the crawl job registry and rate-limit windows.
pub struct AppState {
    scraper: Arc<Scraper>,
    jobs: CrawlJobs,
    config: ServeConfig,
    windows: Mutex<HashMap<String, (Instant, u32)>>,
    job_formats: Mutex<HashMap<String, Vec<OutputFormat>>>,
}

impl std::fmt::Debug for AppState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppState")
            .field("scraper", &self.scraper)
            .finish_non_exhaustive()
    }
}

impl AppState {
    /// Creates the state for [`router`].
    pub fn new(scraper: Scraper, config: ServeConfig) -> Arc<Self> {
        let scraper = Arc::new(scraper);
        Arc::new(Self {
            jobs: CrawlJobs::new(scraper.clone()),
            scraper,
            config,
            windows: Mutex::new(HashMap::new()),
            job_formats: Mutex::new(HashMap::new()),
        })
    }

    /// Counts one request against `key`'s budget. Returns the seconds until the window
    /// resets when the budget is spent.
    fn take_budget(&self, key: &str) -> Result<(), u64> {
        let limit = self.config.requests_per_minute;
        if limit == 0 {
            return Ok(());
        }
        let window = Duration::from_secs(60);
        let Ok(mut windows) = self.windows.lock() else {
            return Ok(());
        };
        let now = Instant::now();
        let entry = windows.entry(key.to_string()).or_insert((now, 0));
        if now.duration_since(entry.0) >= window {
            *entry = (now, 0);
        }
        if entry.1 >= limit {
            let left = window.saturating_sub(now.duration_since(entry.0));
            return Err(left.as_secs().max(1));
        }
        entry.1 += 1;
        Ok(())
    }
}

/// Refuses to listen beyond loopback without API keys.
///
/// # Errors
///
/// Returns [`SeoError::Config`] for a non-loopback address with no keys.
pub fn check_bind(addr: SocketAddr, api_keys: &[String]) -> SeoResult<()> {
    if api_keys.is_empty() && !addr.ip().is_loopback() {
        return Err(SeoError::Config(format!(
            "Refusing to listen on {addr} without API keys. Set BLACKSPARROW_API_KEYS or \
             --api-key, or bind to 127.0.0.1."
        )));
    }
    Ok(())
}

/// Builds the router.
pub fn router(state: Arc<AppState>) -> Router {
    let api = Router::new()
        .route("/v1/scrape", post(scrape))
        .route("/v2/scrape", post(scrape))
        .route("/v1/map", post(map_v1))
        .route("/v2/map", post(map_v2))
        .route("/v1/crawl", post(crawl_start))
        .route("/v2/crawl", post(crawl_start))
        .route("/v1/crawl/{id}", get(crawl_status).delete(crawl_cancel))
        .route("/v2/crawl/{id}", get(crawl_status).delete(crawl_cancel))
        .route("/v1/find", post(find))
        .route("/v1/extract", post(extract))
        .route("/v1/interact", post(interact))
        .layer(middleware::from_fn_with_state(state.clone(), authenticate));
    api.route("/health", get(health))
        .layer(DefaultBodyLimit::max(state.config.max_body_bytes))
        .layer(middleware::from_fn(log_request))
        .with_state(state)
}

/// Binds `addr` and serves until Ctrl-C.
///
/// # Errors
///
/// Returns an error when [`check_bind`] refuses the address or the socket cannot be bound.
pub async fn serve(addr: SocketAddr, state: Arc<AppState>) -> SeoResult<()> {
    check_bind(addr, &state.config.api_keys)?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, "blacksparrow API listening");
    axum::serve(listener, router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

/// A JSON error response: `{"success": false, "error": "…"}`.
#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
    retry_after: Option<u64>,
}

impl ApiError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
            retry_after: None,
        }
    }

    fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }
}

impl From<SeoError> for ApiError {
    fn from(err: SeoError) -> Self {
        let status = match &err {
            SeoError::Config(_) | SeoError::Url(_) | SeoError::Serialization(_) => {
                StatusCode::BAD_REQUEST
            }
            SeoError::Network(_) => StatusCode::BAD_GATEWAY,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        Self::new(status, err.to_string())
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut res = (
            self.status,
            Json(json!({ "success": false, "error": self.message })),
        )
            .into_response();
        if let Some(secs) = self.retry_after {
            if let Ok(v) = HeaderValue::from_str(&secs.to_string()) {
                res.headers_mut().insert("retry-after", v);
            }
        }
        res
    }
}

type ApiResult = Result<Json<Value>, ApiError>;

/// Constant-time comparison so response timing does not reveal key prefixes.
fn same_key(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn presented_key(headers: &HeaderMap) -> Option<&str> {
    if let Some(auth) = headers.get("authorization").and_then(|v| v.to_str().ok()) {
        let auth = auth.trim();
        if let Some(key) = auth
            .strip_prefix("Bearer ")
            .or_else(|| auth.strip_prefix("bearer "))
        {
            return Some(key.trim());
        }
    }
    headers.get("x-api-key").and_then(|v| v.to_str().ok())
}

async fn authenticate(
    State(state): State<Arc<AppState>>,
    req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let budget_key = if state.config.api_keys.is_empty() {
        "anonymous".to_string()
    } else {
        let presented = presented_key(req.headers()).unwrap_or("");
        let index = state
            .config
            .api_keys
            .iter()
            .position(|k| same_key(k, presented))
            .ok_or_else(|| {
                ApiError::new(
                    StatusCode::UNAUTHORIZED,
                    "Missing or invalid API key (Authorization: Bearer <key>)",
                )
            })?;
        format!("key{index}")
    };
    if let Err(secs) = state.take_budget(&budget_key) {
        let mut err = ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            format!("Rate limit exceeded; retry in {secs} s"),
        );
        err.retry_after = Some(secs);
        return Err(err);
    }
    Ok(next.run(req).await)
}

async fn log_request(req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let started = Instant::now();
    let res = next.run(req).await;
    tracing::info!(
        target: "blacksparrow::serve",
        %method,
        path,
        status = res.status().as_u16(),
        ms = started.elapsed().as_millis() as u64,
        "request"
    );
    res
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION") }))
}

fn parse_object(body: &Bytes) -> Result<Map<String, Value>, ApiError> {
    match serde_json::from_slice::<Value>(body) {
        Ok(Value::Object(obj)) => Ok(obj),
        Ok(_) => Err(ApiError::bad_request("Request body must be a JSON object")),
        Err(e) => Err(ApiError::bad_request(format!("Invalid JSON: {e}"))),
    }
}

fn decode<T: DeserializeOwned>(obj: Map<String, Value>) -> Result<T, ApiError> {
    serde_json::from_value(Value::Object(obj))
        .map_err(|e| ApiError::bad_request(format!("Invalid request: {e}")))
}

fn required_url(obj: &Map<String, Value>) -> Result<String, ApiError> {
    obj.get("url")
        .and_then(Value::as_str)
        .filter(|u| !u.trim().is_empty())
        .map(|u| u.trim().to_string())
        .ok_or_else(|| ApiError::bad_request("Missing required field: url"))
}

/// Refuses URLs the scraper would never fetch, with 400 for malformed URLs and 403 for
/// private or metadata addresses.
async fn guard(state: &AppState, url: &str) -> Result<(), ApiError> {
    let config = state.scraper.config();
    match validate_url_safety(
        url,
        config.allow_all_private_ips,
        &config.allowed_private_hosts,
    )
    .await
    {
        Ok(_) => Ok(()),
        Err(SeoError::Url(msg)) => Err(ApiError::bad_request(msg)),
        Err(e) => Err(ApiError::new(StatusCode::FORBIDDEN, e.to_string())),
    }
}

fn base_url(state: &AppState, headers: &HeaderMap) -> String {
    if let Some(public) = &state.config.public_url {
        return public.trim_end_matches('/').to_string();
    }
    let host = headers
        .get("x-forwarded-host")
        .or_else(|| headers.get("host"))
        .and_then(|v| v.to_str().ok())
        .unwrap_or("localhost");
    let proto = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("http");
    format!("{proto}://{host}")
}

fn api_version(uri: &Uri) -> &'static str {
    if uri.path().starts_with("/v2/") {
        "v2"
    } else {
        "v1"
    }
}

async fn scrape(State(state): State<Arc<AppState>>, body: Bytes) -> ApiResult {
    let mut obj = parse_object(&body)?;
    let url = required_url(&obj)?;
    normalize_scrape(&mut obj).map_err(ApiError::bad_request)?;
    let opts: ScrapeOptions = decode(obj)?;
    guard(&state, &url).await?;
    let doc = state.scraper.scrape(&url, &opts).await?;
    if doc.status_code == 0 && doc.status != PageStatus::Ok {
        let message = doc
            .error
            .clone()
            .unwrap_or_else(|| format!("Page could not be scraped ({})", doc.status.as_str()));
        let status = if doc.status == PageStatus::Blocked {
            StatusCode::FORBIDDEN
        } else {
            StatusCode::BAD_GATEWAY
        };
        return Err(ApiError::new(status, message));
    }
    Ok(Json(
        json!({ "success": true, "data": document_json(&doc, &opts.formats) }),
    ))
}

async fn map_v1(state: State<Arc<AppState>>, body: Bytes) -> ApiResult {
    map(state, body, false).await
}

async fn map_v2(state: State<Arc<AppState>>, body: Bytes) -> ApiResult {
    map(state, body, true).await
}

async fn map(State(state): State<Arc<AppState>>, body: Bytes, objects: bool) -> ApiResult {
    let mut obj = parse_object(&body)?;
    let url = required_url(&obj)?;
    normalize_sitemap(&mut obj);
    let opts: MapOptions = decode(obj)?;
    guard(&state, &url).await?;
    let result = map_site(&state.scraper, &url, &opts).await?;
    let links: Vec<Value> = if objects {
        result
            .links
            .iter()
            .map(|l| {
                let mut o = json!({ "url": l.url });
                if let Some(title) = &l.title {
                    o["title"] = json!(title);
                }
                o
            })
            .collect()
    } else {
        result.links.iter().map(|l| json!(l.url)).collect()
    };
    Ok(Json(json!({ "success": true, "links": links })))
}

async fn crawl_start(
    State(state): State<Arc<AppState>>,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult {
    let mut obj = parse_object(&body)?;
    let url = required_url(&obj)?;
    normalize_sitemap(&mut obj);
    normalize_nested(&mut obj).map_err(ApiError::bad_request)?;
    let mut opts: CrawlOptions = decode(obj)?;
    opts.limit = opts.limit.clamp(1, state.config.max_crawl_pages.max(1));
    opts.concurrency = opts.concurrency.clamp(1, 16);
    guard(&state, &url).await?;
    if state.jobs.active_count() >= state.config.max_concurrent_crawls {
        return Err(ApiError::new(
            StatusCode::TOO_MANY_REQUESTS,
            format!(
                "{} crawls are already running; wait for one to finish",
                state.config.max_concurrent_crawls
            ),
        ));
    }
    let formats = opts.scrape.formats.clone();
    let id = state.jobs.start(&url, opts)?;
    if let Ok(mut map) = state.job_formats.lock() {
        map.insert(id.clone(), formats);
    }
    let status_url = format!(
        "{}/{}/crawl/{id}",
        base_url(&state, &headers),
        api_version(&uri)
    );
    Ok(Json(
        json!({ "success": true, "id": id, "url": status_url }),
    ))
}

#[derive(Debug, Deserialize)]
struct PageQuery {
    #[serde(alias = "offset")]
    skip: Option<usize>,
    limit: Option<usize>,
}

fn firecrawl_state(state: CrawlState) -> &'static str {
    match state {
        CrawlState::Queued | CrawlState::Crawling => "scraping",
        CrawlState::Completed => "completed",
        CrawlState::Cancelled => "cancelled",
        CrawlState::Failed => "failed",
    }
}

async fn crawl_status(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(page): Query<PageQuery>,
    uri: Uri,
    headers: HeaderMap,
) -> ApiResult {
    let skip = page.skip.unwrap_or(0);
    let limit = page.limit.unwrap_or(100).clamp(1, 1000);
    let status = state
        .jobs
        .status(&id, skip, limit)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, format!("Unknown crawl '{id}'")))?;
    let formats = state
        .job_formats
        .lock()
        .ok()
        .and_then(|m| m.get(&id).cloned())
        .unwrap_or_else(|| ScrapeOptions::default().formats);
    let data: Vec<Value> = status
        .documents
        .iter()
        .map(|d| document_json(d, &formats))
        .collect();
    let next = status.next.map(|n| {
        let mut next = format!(
            "{}/{}/crawl/{id}?skip={n}",
            base_url(&state, &headers),
            api_version(&uri)
        );
        if page.limit.is_some() {
            next.push_str(&format!("&limit={limit}"));
        }
        next
    });
    let total = if status.state.is_terminal() {
        status.total
    } else {
        status.total.max(status.discovered as usize)
    };
    let mut body = json!({
        "success": true,
        "status": firecrawl_state(status.state),
        "total": total,
        "completed": status.pages_done,
        "failed": status.pages_failed,
        "skipped": status.pages_skipped,
        "creditsUsed": status.pages_done,
        "next": next,
        "data": data,
    });
    if let Some(error) = status.error {
        body["error"] = json!(error);
    }
    Ok(Json(body))
}

async fn crawl_cancel(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> ApiResult {
    if state.jobs.cancel(&id) {
        return Ok(Json(json!({ "success": true, "status": "cancelled" })));
    }
    match state.jobs.status(&id, 0, 0) {
        Some(_) => Err(ApiError::new(
            StatusCode::CONFLICT,
            format!("Crawl '{id}' has already finished"),
        )),
        None => Err(ApiError::new(
            StatusCode::NOT_FOUND,
            format!("Unknown crawl '{id}'"),
        )),
    }
}

async fn find(State(state): State<Arc<AppState>>, body: Bytes) -> ApiResult {
    let mut obj = parse_object(&body)?;
    normalize_nested(&mut obj).map_err(ApiError::bad_request)?;
    let req: FindRequest = decode(obj)?;
    let result = if let Some(url) = &req.url {
        guard(&state, url).await?;
        find_on_page(&state.scraper, &req).await?
    } else if req.crawl_id.is_some() || req.host.is_some() || req.url_prefix.is_some() {
        let db = state.scraper.database().ok_or_else(|| {
            ApiError::bad_request("Searching stored crawls needs a server database")
        })?;
        let conn = db.connect()?;
        let hits = find_in_crawl(&conn, &req)?;
        FindResult {
            mode: if req.regex.is_some() {
                "regex"
            } else {
                "query"
            }
            .to_string(),
            hits,
            ..Default::default()
        }
    } else {
        return Err(ApiError::bad_request(
            "Give a url, or a crawlId / host / urlPrefix to search stored pages",
        ));
    };
    Ok(Json(json!({ "success": true, "data": result })))
}

async fn extract(State(state): State<Arc<AppState>>, body: Bytes) -> ApiResult {
    let mut obj = parse_object(&body)?;
    normalize_nested(&mut obj).map_err(ApiError::bad_request)?;
    let mut req: ExtractRequest = decode(obj)?;
    req.limit = req.limit.clamp(1, state.config.max_crawl_pages.max(1));
    for url in req.url.iter().chain(req.urls.iter()) {
        guard(&state, url).await?;
    }
    let results = extract_fields(&state.scraper, &req).await?;
    let data = match results.as_slice() {
        [one] => one.data.clone(),
        many => Value::Array(many.iter().map(|r| r.data.clone()).collect()),
    };
    Ok(Json(
        json!({ "success": true, "data": data, "results": results }),
    ))
}

async fn interact(State(state): State<Arc<AppState>>, body: Bytes) -> ApiResult {
    let mut obj = parse_object(&body)?;
    let url = required_url(&obj)?;
    normalize_nested(&mut obj).map_err(ApiError::bad_request)?;
    let mut screenshot = false;
    for key in ["actions", "steps"] {
        if let Some(actions) = obj.remove(key) {
            let converted =
                convert_actions(&actions, &mut screenshot).map_err(ApiError::bad_request)?;
            obj.insert(
                "actions".into(),
                serde_json::to_value(converted).map_err(SeoError::from)?,
            );
        }
    }
    let mut req: InteractRequest = decode(obj)?;
    req.screenshot |= screenshot;
    guard(&state, &url).await?;
    let result = interact_page(&state.scraper, &req).await?;
    let mut formats = req.scrape.formats.clone();
    if req.screenshot && !formats.contains(&OutputFormat::Screenshot) {
        formats.push(OutputFormat::Screenshot);
    }
    Ok(Json(json!({
        "success": true,
        "data": document_json(&result.document, &formats),
        "snapshot": result.snapshot,
        "actionErrors": result.action_errors,
        "blockedRequests": result.blocked_requests,
    })))
}
