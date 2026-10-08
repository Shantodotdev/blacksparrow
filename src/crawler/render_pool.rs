//! # Agent-Mode Browser Pool
//!
//! One shared Chrome (launched locally or reached over `--chrome-ws`) with a semaphore-bounded
//! pool of tabs, so several pages render at once. Every request Chrome makes, including
//! redirects and subresources, is intercepted through the CDP `Fetch` domain and checked with
//! the same private-network guard as [`crate::crawler::client::HttpClient`]; requests that fail
//! the check are aborted. Images, fonts, media and known trackers are skipped by default.
//!
//! Before the DOM is serialized, a script marks elements that are invisible to a person
//! (computed `display:none`, `visibility:hidden`, zero opacity, zero font size, off-screen) so
//! the cleaner drops them like statically hidden text.

use crate::core::url::validate_url_safety;
use crate::error::{SeoError, SeoResult};
use crate::extract::types::{BrowserAction, WaitUntil};
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::fetch::{
    ContinueRequestParams, EnableParams, EventRequestPaused, FailRequestParams,
};
use chromiumoxide::cdp::browser_protocol::input::InsertTextParams;
use chromiumoxide::cdp::browser_protocol::network::{
    ErrorReason, Headers, ResourceType, SetExtraHttpHeadersParams,
};
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::page::ScreenshotParams;
use chromiumoxide::Page;
use futures::StreamExt;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};
use tokio::sync::Semaphore;

/// Attribute set on elements the hidden-text script found invisible.
pub const HIDDEN_MARKER_ATTR: &str = "data-bs-hidden";
/// Attribute holding accessibility snapshot references (`e12`).
pub const REF_ATTR: &str = "data-bs-ref";

/// Extra time after the render limit to read a page whose steps ran out of time.
const READ_GRACE: Duration = Duration::from_secs(5);
/// How long a local Chrome may take to start.
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(45);
/// Local Chrome start attempts before giving up.
const LAUNCH_ATTEMPTS: u32 = 2;

const TRACKER_HOSTS: &[&str] = &[
    "google-analytics.com",
    "googletagmanager.com",
    "doubleclick.net",
    "facebook.net",
    "hotjar.com",
    "segment.io",
    "segment.com",
    "mixpanel.com",
    "clarity.ms",
    "scorecardresearch.com",
];

/// Settings for launching or connecting to Chrome.
#[derive(Debug, Clone)]
pub struct RenderPoolConfig {
    /// Remote CDP endpoint; `None` launches a local Chrome.
    pub chrome_ws: Option<String>,
    /// Maximum tabs rendering at once.
    pub max_tabs: usize,
    /// User-Agent for every tab.
    pub user_agent: String,
    /// Extra request headers.
    pub headers: Vec<(String, String)>,
    /// Proxy for a locally launched Chrome.
    pub proxy: Option<String>,
    /// Allow private and local addresses (cloud metadata stays blocked).
    pub allow_all_private_ips: bool,
    /// Specific private `host` or `host:port` entries allowed.
    pub allowed_private_hosts: Vec<String>,
    /// Skip images, fonts, media and trackers.
    pub block_resources: bool,
}

impl Default for RenderPoolConfig {
    fn default() -> Self {
        Self {
            chrome_ws: None,
            max_tabs: 4,
            user_agent: crate::core::branding::DEFAULT_USER_AGENT.to_string(),
            headers: Vec::new(),
            proxy: None,
            allow_all_private_ips: false,
            allowed_private_hosts: Vec::new(),
            block_resources: true,
        }
    }
}

/// What to do in a tab before reading it.
#[derive(Debug, Clone, Default)]
pub struct RenderRequest {
    /// Readiness condition after navigation.
    pub wait_until: WaitUntil,
    /// Selector that must appear.
    pub wait_for_selector: Option<String>,
    /// Extra fixed wait in milliseconds.
    pub wait_ms: Option<u64>,
    /// Steps to run.
    pub actions: Vec<BrowserAction>,
    /// Capture a full-page PNG.
    pub screenshot: bool,
    /// Return an accessibility snapshot of interactive elements.
    pub snapshot: bool,
    /// Overall time limit (default 30 s).
    pub timeout: Option<Duration>,
}

/// One interactive element of an accessibility snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotElement {
    /// Stable reference usable as an action target (`e12`).
    #[serde(rename = "ref")]
    pub reference: String,
    /// ARIA-style role (`button`, `link`, `textbox`, ...).
    pub role: String,
    /// Accessible name.
    pub name: String,
}

impl std::fmt::Display for SnapshotElement {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {} {:?}", self.reference, self.role, self.name)
    }
}

/// Result of rendering one page.
#[derive(Debug, Clone, Default)]
pub struct RenderOutput {
    /// Serialized DOM, with invisible elements marked by [`HIDDEN_MARKER_ATTR`].
    pub html: String,
    /// URL after redirects and client-side navigation.
    pub final_url: String,
    /// PNG bytes when a screenshot was requested.
    pub screenshot_png: Option<Vec<u8>>,
    /// Interactive elements when a snapshot was requested.
    pub snapshot: Vec<SnapshotElement>,
    /// Requests aborted by the network guard.
    pub blocked_requests: Vec<String>,
    /// Action failures (the page is still returned).
    pub action_errors: Vec<String>,
}

/// A shared browser with a bounded pool of tabs.
pub struct RenderPool {
    browser: Browser,
    handler_task: tokio::task::JoinHandle<()>,
    profile_dir: Option<tempfile::TempDir>,
    owns_browser: bool,
    permits: Arc<Semaphore>,
    config: RenderPoolConfig,
}

impl std::fmt::Debug for RenderPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderPool")
            .field("max_tabs", &self.config.max_tabs)
            .field("owns_browser", &self.owns_browser)
            .finish()
    }
}

impl RenderPool {
    /// Launches a local Chrome (or connects to `chrome_ws`).
    ///
    /// # Errors
    ///
    /// Returns [`SeoError::Network`] when no browser can be started or reached.
    pub async fn launch(config: RenderPoolConfig) -> SeoResult<Self> {
        let mut profile_dir = None;
        let owns_browser = config.chrome_ws.is_none();
        let connection = match config.chrome_ws.as_deref() {
            Some(endpoint) => Browser::connect(endpoint.to_string()).await,
            None => {
                // A cold Chrome on a busy machine can take longer than chromiumoxide's 20 s
                // default to print its DevTools URL, so wait longer and try a second time.
                let mut attempt = 0;
                loop {
                    attempt += 1;
                    let profile = tempfile::Builder::new()
                        .prefix("blacksparrow-agent-chrome-")
                        .tempdir()
                        .map_err(SeoError::Io)?;
                    let mut builder = BrowserConfig::builder()
                        .new_headless_mode()
                        .no_sandbox()
                        .launch_timeout(LAUNCH_TIMEOUT)
                        .user_data_dir(profile.path())
                        .arg("--disable-gpu")
                        .arg("--disable-dev-shm-usage")
                        .arg("--mute-audio")
                        // Tabs render side by side; stop Chrome from throttling the ones
                        // that are not in front.
                        .arg("--disable-background-timer-throttling")
                        .arg("--disable-backgrounding-occluded-windows")
                        .arg("--disable-renderer-backgrounding");
                    if let Some(proxy_url) = config.proxy.as_deref() {
                        builder = builder.arg(format!("--proxy-server={proxy_url}"));
                    }
                    let browser_config = builder.build().map_err(|error| {
                        SeoError::Config(format!("Invalid Chrome configuration: {error}"))
                    })?;
                    match Browser::launch(browser_config).await {
                        Ok(launched) => {
                            profile_dir = Some(profile);
                            break Ok(launched);
                        }
                        Err(error) if attempt >= LAUNCH_ATTEMPTS => break Err(error),
                        Err(_) => continue,
                    }
                }
            }
        }
        .map_err(|error| {
            let source = config
                .chrome_ws
                .as_deref()
                .unwrap_or("an automatically detected local browser");
            SeoError::Network(format!("Unable to start Chrome through {source}: {error}"))
        })?;

        let (browser, mut handler) = connection;
        let handler_task = tokio::spawn(async move {
            while let Some(result) = handler.next().await {
                if result.is_err() {
                    break;
                }
            }
        });

        Ok(Self {
            browser,
            handler_task,
            profile_dir,
            owns_browser,
            permits: Arc::new(Semaphore::new(config.max_tabs.max(1))),
            config,
        })
    }

    /// Maximum tabs rendering at once.
    pub fn max_tabs(&self) -> usize {
        self.config.max_tabs.max(1)
    }

    /// Renders `url` in a fresh tab and returns the DOM after the request's wait conditions
    /// and actions.
    ///
    /// # Errors
    ///
    /// Returns [`SeoError::Network`] when the URL fails the network guard, navigation fails,
    /// or the time limit is exceeded.
    pub async fn render(&self, url: &str, request: &RenderRequest) -> SeoResult<RenderOutput> {
        validate_url_safety(
            url,
            self.config.allow_all_private_ips,
            &self.config.allowed_private_hosts,
        )
        .await?;

        let _permit = self
            .permits
            .acquire()
            .await
            .map_err(|_| SeoError::Internal("Render pool closed".to_string()))?;
        let page = self
            .browser
            .new_page("about:blank")
            .await
            .map_err(|e| SeoError::Network(format!("Unable to open a Chrome tab: {e}")))?;

        let blocked = Arc::new(StdMutex::new(Vec::new()));
        let guard_task = self
            .install_guard(&page, Arc::clone(&blocked), request.screenshot)
            .await;
        let limit = request.timeout.unwrap_or(Duration::from_secs(30));
        let result = match guard_task {
            Ok(task) => {
                // Steps stop at `limit`; the grace period lets a page whose step timed out
                // still be read and returned with the step error.
                let outcome = tokio::time::timeout(
                    limit + READ_GRACE,
                    self.drive(&page, url, request, limit),
                )
                .await;
                task.abort();
                match outcome {
                    Ok(inner) => inner,
                    Err(_) => Err(SeoError::Network(format!(
                        "Rendering {url} exceeded {} ms",
                        (limit + READ_GRACE).as_millis()
                    ))),
                }
            }
            Err(e) => Err(e),
        };
        let _ = page.close().await;

        let mut output = result?;
        output.blocked_requests = blocked.lock().map(|b| b.clone()).unwrap_or_default();
        Ok(output)
    }

    async fn install_guard(
        &self,
        page: &Page,
        blocked: Arc<StdMutex<Vec<String>>>,
        keep_images: bool,
    ) -> SeoResult<tokio::task::JoinHandle<()>> {
        page.set_user_agent(self.config.user_agent.as_str())
            .await
            .map_err(|e| SeoError::Network(format!("Unable to set Chrome user agent: {e}")))?;
        if !self.config.headers.is_empty() {
            let headers: BTreeMap<String, String> = self.config.headers.iter().cloned().collect();
            page.execute(SetExtraHttpHeadersParams::new(Headers::new(
                serde_json::json!(headers),
            )))
            .await
            .map_err(|e| SeoError::Network(format!("Unable to set Chrome headers: {e}")))?;
        }

        let mut paused = page
            .event_listener::<EventRequestPaused>()
            .await
            .map_err(|e| SeoError::Internal(format!("Unable to intercept requests: {e}")))?;
        let task_page = page.clone();
        let allow_all = self.config.allow_all_private_ips;
        let allowed_hosts = self.config.allowed_private_hosts.clone();
        let block_resources = self.config.block_resources;
        let task = tokio::spawn(async move {
            while let Some(event) = paused.next().await {
                let request_url = event.request.url.clone();
                let verdict = request_verdict(
                    &request_url,
                    &event.resource_type,
                    allow_all,
                    &allowed_hosts,
                    block_resources,
                    keep_images,
                )
                .await;
                let id = event.request_id.clone();
                match verdict {
                    Verdict::Continue => {
                        let _ = task_page.execute(ContinueRequestParams::new(id)).await;
                    }
                    Verdict::Skip => {
                        let _ = task_page
                            .execute(FailRequestParams::new(id, ErrorReason::BlockedByClient))
                            .await;
                    }
                    Verdict::Unsafe => {
                        if let Ok(mut list) = blocked.lock() {
                            list.push(request_url);
                        }
                        let _ = task_page
                            .execute(FailRequestParams::new(id, ErrorReason::BlockedByClient))
                            .await;
                    }
                }
            }
        });
        page.execute(EnableParams::default())
            .await
            .map_err(|e| SeoError::Internal(format!("Unable to enable request guard: {e}")))?;
        Ok(task)
    }

    async fn drive(
        &self,
        page: &Page,
        url: &str,
        request: &RenderRequest,
        limit: Duration,
    ) -> SeoResult<RenderOutput> {
        let deadline = Instant::now() + limit;
        page.goto(url)
            .await
            .map_err(|e| SeoError::Network(format!("Chrome failed to load {url}: {e}")))?;

        if request.wait_until == WaitUntil::NetworkIdle {
            wait_network_idle(page, deadline, Duration::from_millis(500)).await;
        }
        if let Some(selector) = request.wait_for_selector.as_deref() {
            wait_for_selector(page, selector, deadline).await?;
        }
        if let Some(ms) = request.wait_ms {
            tokio::time::sleep(Duration::from_millis(ms.min(30_000))).await;
        }

        let mut output = RenderOutput::default();
        if !request.actions.is_empty() {
            // References from a snapshot must exist before actions can target them.
            let _ = snapshot(page).await;
            for (index, action) in request.actions.iter().enumerate() {
                if let Err(e) = run_action(page, action, deadline).await {
                    output
                        .action_errors
                        .push(format!("step {}: {e}", index + 1));
                    break;
                }
            }
        }

        if request.snapshot {
            output.snapshot = snapshot(page).await.unwrap_or_default();
        }
        if request.screenshot {
            let shot = page
                .screenshot(
                    ScreenshotParams::builder()
                        .format(CaptureScreenshotFormat::Png)
                        .full_page(true)
                        .build(),
                )
                .await
                .map_err(|e| SeoError::Internal(format!("Screenshot failed: {e}")))?;
            output.screenshot_png = Some(shot);
        }

        let _ = page.evaluate(MARK_HIDDEN_SCRIPT).await;
        output.html = page
            .content()
            .await
            .map_err(|e| SeoError::Internal(format!("Unable to serialize rendered DOM: {e}")))?;
        output.final_url = page
            .url()
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| url.to_string());
        Ok(output)
    }

    /// Closes a browser this pool launched.
    ///
    /// # Errors
    ///
    /// Returns [`SeoError::Internal`] when Chrome cannot be closed cleanly.
    pub async fn shutdown(mut self) -> SeoResult<()> {
        if self.owns_browser {
            self.browser
                .close()
                .await
                .map_err(|e| SeoError::Internal(format!("Unable to close Chrome: {e}")))?;
        }
        self.handler_task.abort();
        drop(self.profile_dir.take());
        Ok(())
    }
}

enum Verdict {
    Continue,
    Skip,
    Unsafe,
}

async fn request_verdict(
    url: &str,
    resource: &ResourceType,
    allow_all: bool,
    allowed_hosts: &[String],
    block_resources: bool,
    keep_images: bool,
) -> Verdict {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("data:") || lower.starts_with("blob:") || lower.starts_with("about:") {
        return Verdict::Continue;
    }
    if validate_url_safety(url, allow_all, allowed_hosts)
        .await
        .is_err()
    {
        return Verdict::Unsafe;
    }
    if block_resources {
        let heavy = match resource {
            ResourceType::Image => !keep_images,
            ResourceType::Font | ResourceType::Media => true,
            _ => false,
        };
        let host = url::Url::parse(url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_ascii_lowercase))
            .unwrap_or_default();
        let tracker = TRACKER_HOSTS
            .iter()
            .any(|t| host == *t || host.ends_with(&format!(".{t}")));
        if heavy || tracker {
            return Verdict::Skip;
        }
    }
    Verdict::Continue
}

async fn eval_bool(page: &Page, script: String) -> bool {
    page.evaluate(script)
        .await
        .ok()
        .and_then(|v| v.into_value::<bool>().ok())
        .unwrap_or(false)
}

async fn wait_network_idle(page: &Page, deadline: Instant, quiet: Duration) {
    let mut last_count: i64 = -1;
    let mut stable_since = Instant::now();
    while Instant::now() < deadline {
        let count = page
            .evaluate("performance.getEntriesByType('resource').length")
            .await
            .ok()
            .and_then(|v| v.into_value::<i64>().ok())
            .unwrap_or(0);
        if count != last_count {
            last_count = count;
            stable_since = Instant::now();
        } else if stable_since.elapsed() >= quiet {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn selector_for(target: &str) -> String {
    let t = target.trim();
    if t.len() > 1 && t.starts_with('e') && t[1..].chars().all(|c| c.is_ascii_digit()) {
        format!("[{REF_ATTR}=\"{t}\"]")
    } else {
        t.to_string()
    }
}

fn js_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}

async fn wait_for_selector(page: &Page, selector: &str, deadline: Instant) -> SeoResult<()> {
    let script = format!(
        "(() => {{ try {{ return document.querySelector({}) !== null; }} catch (e) {{ return false; }} }})()",
        js_string(&selector_for(selector))
    );
    while Instant::now() < deadline {
        if eval_bool(page, script.clone()).await {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err(SeoError::Network(format!(
        "Timed out waiting for selector '{selector}'"
    )))
}

async fn run_action(page: &Page, action: &BrowserAction, deadline: Instant) -> SeoResult<()> {
    let step_deadline = deadline.min(Instant::now() + Duration::from_secs(10));
    match action {
        BrowserAction::Click { target } => {
            wait_for_selector(page, target, step_deadline).await?;
            let script = format!(
                "(() => {{ const el = document.querySelector({}); if (!el) return false; el.scrollIntoView({{block: 'center'}}); el.click(); return true; }})()",
                js_string(&selector_for(target))
            );
            if !eval_bool(page, script).await {
                return Err(SeoError::Network(format!("Nothing to click at '{target}'")));
            }
            settle(page, step_deadline).await;
        }
        BrowserAction::Type { target, text } => {
            wait_for_selector(page, target, step_deadline).await?;
            let script = format!(
                "(() => {{ const el = document.querySelector({}); if (!el) return false; el.focus(); return true; }})()",
                js_string(&selector_for(target))
            );
            if !eval_bool(page, script).await {
                return Err(SeoError::Network(format!(
                    "Nothing to type into at '{target}'"
                )));
            }
            page.execute(InsertTextParams::new(text.clone()))
                .await
                .map_err(|e| SeoError::Network(format!("Typing failed: {e}")))?;
        }
        BrowserAction::Press { key } => {
            let element = match page.find_element(":focus").await {
                Ok(el) => el,
                Err(_) => page
                    .find_element("body")
                    .await
                    .map_err(|e| SeoError::Network(format!("No element to press keys on: {e}")))?,
            };
            element
                .press_key(key)
                .await
                .map_err(|e| SeoError::Network(format!("Key press '{key}' failed: {e}")))?;
            settle(page, step_deadline).await;
        }
        BrowserAction::Scroll { times } => {
            for _ in 0..(*times).clamp(1, 50) {
                let _ = page
                    .evaluate("window.scrollTo(0, document.documentElement.scrollHeight)")
                    .await;
                settle(page, step_deadline).await;
            }
        }
        BrowserAction::WaitFor { selector } => {
            wait_for_selector(page, selector, step_deadline).await?;
        }
        BrowserAction::Wait { ms } => {
            tokio::time::sleep(Duration::from_millis((*ms).min(30_000))).await;
        }
    }
    Ok(())
}

/// Short network-idle wait after an interaction.
async fn settle(page: &Page, deadline: Instant) {
    let short = deadline.min(Instant::now() + Duration::from_secs(3));
    tokio::time::sleep(Duration::from_millis(100)).await;
    wait_network_idle(page, short, Duration::from_millis(300)).await;
}

/// Tags visible interactive elements with stable references and lists them.
async fn snapshot(page: &Page) -> SeoResult<Vec<SnapshotElement>> {
    let value = page
        .evaluate(SNAPSHOT_SCRIPT)
        .await
        .map_err(|e| SeoError::Internal(format!("Snapshot failed: {e}")))?;
    value
        .into_value::<Vec<SnapshotElement>>()
        .map_err(|e| SeoError::Internal(format!("Snapshot decode failed: {e}")))
}

const SNAPSHOT_SCRIPT: &str = r#"
(() => {
  const ATTR = 'data-bs-ref';
  let next = 1;
  for (const el of document.querySelectorAll('[' + ATTR + ']')) {
    const n = parseInt(el.getAttribute(ATTR).slice(1), 10);
    if (n >= next) next = n + 1;
  }
  const selector = 'a[href], button, input:not([type=hidden]), select, textarea, summary, ' +
    '[role=button], [role=link], [role=checkbox], [role=tab], [role=menuitem], [onclick], [contenteditable=true]';
  const roleOf = (el) => {
    const explicit = el.getAttribute('role');
    if (explicit) return explicit;
    const tag = el.tagName.toLowerCase();
    if (tag === 'a') return 'link';
    if (tag === 'button' || tag === 'summary') return 'button';
    if (tag === 'select') return 'combobox';
    if (tag === 'textarea') return 'textbox';
    if (tag === 'input') {
      const t = (el.getAttribute('type') || 'text').toLowerCase();
      if (t === 'checkbox' || t === 'radio') return t;
      if (t === 'submit' || t === 'button' || t === 'reset') return 'button';
      return 'textbox';
    }
    return 'button';
  };
  const visible = (el) => {
    const s = getComputedStyle(el);
    if (s.display === 'none' || s.visibility === 'hidden' || parseFloat(s.opacity) === 0) return false;
    const r = el.getBoundingClientRect();
    return r.width > 0 && r.height > 0;
  };
  const out = [];
  for (const el of document.querySelectorAll(selector)) {
    if (!visible(el)) continue;
    let ref = el.getAttribute(ATTR);
    if (!ref) { ref = 'e' + (next++); el.setAttribute(ATTR, ref); }
    const name = (el.getAttribute('aria-label') || el.innerText || el.value ||
      el.getAttribute('placeholder') || el.getAttribute('title') || el.getAttribute('alt') || '')
      .replace(/\s+/g, ' ').trim().slice(0, 80);
    out.push({ ref, role: roleOf(el), name });
    if (out.length >= 300) break;
  }
  return out;
})()
"#;

const MARK_HIDDEN_SCRIPT: &str = r#"
(() => {
  const ATTR = 'data-bs-hidden';
  const root = document.body;
  if (!root) return 0;
  let marked = 0;
  const walk = (el) => {
    const s = getComputedStyle(el);
    if (s.display === 'contents') { for (const c of el.children) walk(c); return; }
    const r = el.getBoundingClientRect();
    const hasText = (el.textContent || '').trim().length > 0;
    const hidden = s.display === 'none' || s.visibility === 'hidden' || s.visibility === 'collapse' ||
      parseFloat(s.opacity) === 0 || (hasText && parseFloat(s.fontSize) === 0) ||
      (r.right + window.scrollX <= 0 && r.width > 0) || (r.bottom + window.scrollY <= 0 && r.height > 0) ||
      (hasText && (r.width === 0 || r.height === 0) && s.overflow === 'hidden');
    if (hidden) { el.setAttribute(ATTR, '1'); marked++; return; }
    for (const c of el.children) walk(c);
  };
  for (const c of root.children) walk(c);
  return marked;
})()
"#;
