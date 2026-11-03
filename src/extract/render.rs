//! # Client-Side Rendering (Headless Chromium over CDP)
//!
//! Renders pages whose content only exists after JavaScript runs. Chromium is never bundled: it
//! is launched from a configured executable (or reached over a DevTools WebSocket) through
//! `chromiumoxide`, only when a page needs it, and shut down at the end of the run.
//!
//! Every request the page makes is paused and checked before it leaves the browser: images,
//! media and fonts are refused (they cost memory and CPU and carry no text), and any other request
//! must pass the same [`AddressPolicy`] as plain fetches, so a page cannot use the browser to reach
//! private or cloud-metadata addresses. Each run gets its own temporary profile directory, which
//! is removed on close.

use super::guard::{parse_web_url, AddressPolicy};
use crate::error::{SeoError, SeoResult};
use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::fetch::{
    ContinueRequestParams, EventRequestPaused, FailRequestParams,
};
use chromiumoxide::cdp::browser_protocol::network::{ErrorReason, ResourceType};
use chromiumoxide::Page;
use futures::StreamExt;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::task::JoinHandle;
use url::Url;

/// Longest a rendered page may wait for its text to stop changing after load.
const SETTLE_LIMIT: Duration = Duration::from_secs(5);
const SETTLE_POLL: Duration = Duration::from_millis(300);
/// Most CSSOM text read from a rendered page (CSS-in-JS rules).
const MAX_CSSOM_CHARS: usize = 2_000_000;

/// Low-footprint switches for a crawler that only needs the DOM.
const LEAN_ARGS: &[&str] = &[
    "--disable-gpu",
    "--disable-dev-shm-usage",
    "--disable-software-rasterizer",
    "--disable-extensions",
    "--disable-background-networking",
    "--disable-component-update",
    "--disable-default-apps",
    "--disable-sync",
    // One flag only: Chromium reads the last --disable-features. Site isolation is off because
    // each run has a fresh profile with no cookies or logins to protect, and it costs a renderer
    // process per cross-site frame (ads, video embeds).
    "--disable-features=Translate,MediaRouter,OptimizationHints,AutofillServerCommunication,InterestFeedContentSuggestions,CalculateNativeWinOcclusion,IsolateOrigins,site-per-process",
    "--disable-site-isolation-trials",
    "--blink-settings=imagesEnabled=false",
    "--renderer-process-limit=1",
    "--js-flags=--max-old-space-size=384",
    "--mute-audio",
    "--no-first-run",
    "--no-default-browser-check",
    "--password-store=basic",
    "--use-mock-keychain",
    "--metrics-recording-only",
];

/// Where and how to run Chromium.
#[derive(Debug, Clone, Default)]
pub struct RenderOptions {
    /// Chromium executable; detected on the `PATH` when absent.
    pub chrome_path: Option<PathBuf>,
    /// DevTools WebSocket of an already running browser; takes precedence over launching.
    pub chrome_ws: Option<String>,
    /// Extra command-line switches for a launched browser.
    pub extra_args: Vec<String>,
    /// Runs Chromium without its sandbox (needed in most containers without user namespaces).
    pub no_sandbox: bool,
    /// Longest wait for one page's navigation.
    pub page_timeout: Duration,
    /// User-Agent override; the browser's own when `None`.
    pub user_agent: Option<String>,
    /// Address policy applied to every browser request.
    pub policy: Option<AddressPolicy>,
}

/// A rendered page.
#[derive(Debug, Clone, Serialize)]
pub struct Rendered {
    /// The URL after client and server redirects.
    pub final_url: String,
    /// `document.documentElement.outerHTML` after scripts ran.
    pub html: String,
    /// CSS rules the page built in the CSSOM without text in the DOM (CSS-in-JS).
    pub cssom: Vec<String>,
    /// Requests refused (images, fonts, media, refused addresses).
    pub blocked_requests: u32,
    /// Milliseconds the render took.
    pub elapsed_ms: u64,
}

/// A running browser that renders pages one at a time.
pub struct Renderer {
    browser: Browser,
    handler: JoinHandle<()>,
    profile_dir: Option<PathBuf>,
    options: RenderOptions,
}

impl Renderer {
    /// Starts (or connects to) the browser.
    ///
    /// # Errors
    ///
    /// Returns [`SeoError::Config`] when no executable is found and [`SeoError::Network`] when
    /// the browser cannot be started or reached.
    pub async fn launch(options: RenderOptions) -> SeoResult<Self> {
        let (browser, mut handler, profile_dir) = match &options.chrome_ws {
            Some(ws) => {
                let (browser, handler) = Browser::connect(ws.clone())
                    .await
                    .map_err(|e| SeoError::Network(format!("cannot reach Chrome at {ws}: {e}")))?;
                (browser, handler, None)
            }
            None => {
                let nanos = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0);
                let profile_dir = std::env::temp_dir().join(format!(
                    "blacksparrow-chrome-{}-{nanos}",
                    std::process::id()
                ));
                let mut builder = BrowserConfig::builder()
                    .user_data_dir(&profile_dir)
                    .enable_request_intercept()
                    .request_timeout(options.page_timeout)
                    .launch_timeout(Duration::from_secs(20))
                    .window_size(1280, 900)
                    .viewport(None)
                    .args(LEAN_ARGS.iter().copied())
                    .args(options.extra_args.iter().map(String::as_str));
                if let Some(path) = options.chrome_path.as_ref() {
                    builder = builder.chrome_executable(path);
                }
                if options.no_sandbox {
                    builder = builder.no_sandbox();
                } else {
                    builder = builder.arg("--enable-sandbox");
                }
                if let Some(proxy) = proxy_from_env() {
                    builder = builder.arg(format!("--proxy-server={proxy}"));
                }
                let config = builder.build().map_err(|e| {
                    SeoError::Config(format!(
                        "no Chromium executable (pass --chrome-path or set CHROME_PATH): {e}"
                    ))
                })?;
                let (browser, handler) = Browser::launch(config)
                    .await
                    .map_err(|e| SeoError::Network(format!("cannot start Chromium: {e}")))?;
                (browser, handler, Some(profile_dir))
            }
        };
        let handler = tokio::spawn(async move {
            while let Some(event) = handler.next().await {
                if event.is_err() {
                    // A malformed or unknown CDP message is not fatal for the session.
                    continue;
                }
            }
        });
        Ok(Self {
            browser,
            handler,
            profile_dir,
            options,
        })
    }

    /// Renders one page and returns its DOM after scripts settled.
    ///
    /// # Errors
    ///
    /// Returns [`SeoError::Network`] when navigation fails or times out.
    pub async fn render(&self, url: &Url) -> SeoResult<Rendered> {
        let started = Instant::now();
        let page = self
            .browser
            .new_page("about:blank")
            .await
            .map_err(|e| SeoError::Network(format!("cannot open a tab: {e}")))?;
        let result = tokio::time::timeout(
            self.options.page_timeout + SETTLE_LIMIT,
            self.render_in(&page, url, started),
        )
        .await
        .unwrap_or_else(|_| Err(SeoError::Network("render timed out".to_string())));
        let _ = page.close().await;
        result
    }

    async fn render_in(&self, page: &Page, url: &Url, started: Instant) -> SeoResult<Rendered> {
        if let Some(agent) = &self.options.user_agent {
            page.set_user_agent(agent.as_str())
                .await
                .map_err(|e| SeoError::Network(format!("cannot set user agent: {e}")))?;
        }
        let blocked = Arc::new(AtomicU32::new(0));
        let interceptor = self.intercept(page, blocked.clone()).await?;

        let navigation = tokio::time::timeout(self.options.page_timeout, page.goto(url.as_str()))
            .await
            .map_err(|_| SeoError::Network("navigation timed out".to_string()))?;
        if let Err(err) = navigation {
            interceptor.abort();
            return Err(SeoError::Network(format!("navigation failed: {err}")));
        }
        settle(page).await;

        let html = page
            .content()
            .await
            .map_err(|e| SeoError::Network(format!("cannot read the DOM: {e}")))?;
        let cssom: Vec<String> = page
            .evaluate(cssom_script())
            .await
            .ok()
            .and_then(|result| result.into_value().ok())
            .unwrap_or_default();
        let final_url = page
            .url()
            .await
            .ok()
            .flatten()
            .unwrap_or_else(|| url.to_string());
        interceptor.abort();
        Ok(Rendered {
            final_url,
            html,
            cssom,
            blocked_requests: blocked.load(Ordering::Relaxed),
            elapsed_ms: started.elapsed().as_millis() as u64,
        })
    }

    /// Answers every paused request of `page`: refuse heavy resource types and refused addresses,
    /// continue the rest.
    async fn intercept(&self, page: &Page, blocked: Arc<AtomicU32>) -> SeoResult<JoinHandle<()>> {
        let mut events = page
            .event_listener::<EventRequestPaused>()
            .await
            .map_err(|e| SeoError::Network(format!("cannot intercept requests: {e}")))?;
        let page = page.clone();
        let policy = self.options.policy;
        Ok(tokio::spawn(async move {
            while let Some(event) = events.next().await {
                let allowed = request_allowed(&event, policy).await;
                let id = event.request_id.clone();
                if allowed {
                    let _ = page.execute(ContinueRequestParams::new(id)).await;
                } else {
                    blocked.fetch_add(1, Ordering::Relaxed);
                    let _ = page
                        .execute(FailRequestParams::new(id, ErrorReason::BlockedByClient))
                        .await;
                }
            }
        }))
    }

    /// Closes the browser and removes its temporary profile.
    pub async fn close(mut self) {
        let _ = self.browser.close().await;
        let _ = tokio::time::timeout(Duration::from_secs(5), self.browser.wait()).await;
        self.handler.abort();
        if let Some(dir) = self.profile_dir.take() {
            let _ = tokio::fs::remove_dir_all(dir).await;
        }
    }
}

/// Hosts of analytics, advertising and session-recording scripts: they cost CPU and network
/// and never add page content.
const TRACKER_HOSTS: &[&str] = &[
    "googletagmanager.com",
    "google-analytics.com",
    "analytics.google.com",
    "doubleclick.net",
    "googleadservices.com",
    "googlesyndication.com",
    "connect.facebook.net",
    "facebook.com/tr",
    "hotjar.com",
    "clarity.ms",
    "analytics.tiktok.com",
    "snap.licdn.com",
    "static.ads-twitter.com",
    "bat.bing.com",
    "cdn.mxpnl.com",
    "cdn.segment.com",
    "fullstory.com",
    "mouseflow.com",
    "yandex.ru/metrika",
    "mc.yandex.ru",
];

fn is_tracker(url: &Url) -> bool {
    let host = url.host_str().unwrap_or("").to_ascii_lowercase();
    let host_path = format!("{host}{}", url.path());
    TRACKER_HOSTS.iter().any(|tracker| {
        if tracker.contains('/') {
            host_path.starts_with(tracker) || host_path.contains(&format!(".{tracker}"))
        } else {
            host == *tracker || host.ends_with(&format!(".{tracker}"))
        }
    })
}

async fn request_allowed(event: &EventRequestPaused, policy: Option<AddressPolicy>) -> bool {
    if matches!(
        event.resource_type,
        ResourceType::Image
            | ResourceType::Media
            | ResourceType::Font
            | ResourceType::Manifest
            | ResourceType::Ping
            | ResourceType::CspViolationReport
            | ResourceType::Prefetch
            | ResourceType::TextTrack
    ) {
        return false;
    }
    let raw = event.request.url.as_str();
    if raw.starts_with("data:") || raw.starts_with("blob:") {
        return true;
    }
    let Ok(url) = parse_web_url(raw) else {
        return false;
    };
    if is_tracker(&url) {
        return false;
    }
    match policy {
        Some(policy) => policy.check_url(&url).await.is_ok(),
        None => true,
    }
}

/// Waits until the page's text stops growing (client rendering finished) or the limit passes.
async fn settle(page: &Page) {
    let deadline = Instant::now() + SETTLE_LIMIT;
    let mut last: i64 = -1;
    let mut stable = 0u8;
    while Instant::now() < deadline {
        tokio::time::sleep(SETTLE_POLL).await;
        let length: i64 = page
            .evaluate(
                "document.readyState === 'complete' && document.body ? document.body.innerText.length : -1",
            )
            .await
            .ok()
            .and_then(|result| result.into_value().ok())
            .unwrap_or(-1);
        // Carousels and timers keep changing a page's text a little; a change under 2% (or 20
        // characters) is the page at rest.
        let tolerance = (last / 50).max(20);
        if length >= 0 && last >= 0 && (length - last).abs() <= tolerance {
            stable += 1;
            if stable >= 2 {
                return;
            }
        } else {
            stable = 0;
        }
        last = length;
    }
}

fn proxy_from_env() -> Option<String> {
    ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy"]
        .iter()
        .find_map(|key| std::env::var(key).ok())
        .filter(|value| !value.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_tracker_requests_but_not_site_content() {
        let tracker = |raw: &str| is_tracker(&Url::parse(raw).unwrap());
        assert!(tracker("https://www.googletagmanager.com/gtm.js?id=GTM-1"));
        assert!(tracker("https://connect.facebook.net/en_US/fbevents.js"));
        assert!(tracker("https://www.facebook.com/tr?id=1&ev=PageView"));
        assert!(!tracker("https://www.facebook.com/SampleBakery"));
        assert!(!tracker("https://shop.test/static/app.js"));
        assert!(!tracker("https://notgoogletagmanager.com/x.js"));
    }
}

/// Collects rules of stylesheets that have no text in the DOM: constructed sheets and `<style>`
/// elements filled through `insertRule` (CSS-in-JS). Linked sheets are fetched separately.
const CSSOM_SCRIPT: &str = r#"(() => {
  const out = []; let total = 0;
  const sheets = Array.from(document.styleSheets).concat(Array.from(document.adoptedStyleSheets || []));
  for (const sheet of sheets) {
    if (sheet.href) continue;
    const owner = sheet.ownerNode;
    if (owner && owner.textContent && owner.textContent.trim() !== '') continue;
    let rules; try { rules = sheet.cssRules; } catch (e) { continue; }
    if (!rules) continue;
    for (const rule of Array.from(rules)) {
      const text = rule.cssText; total += text.length;
      if (total > MAX) return out;
      out.push(text);
    }
  }
  return out;
})()"#;

/// The CSSOM script with its size limit filled in.
pub fn cssom_script() -> String {
    CSSOM_SCRIPT.replace("MAX", &MAX_CSSOM_CHARS.to_string())
}
