//! Decoupled Chrome DevTools Protocol renderer for JavaScript applications.
//!
//! SEO Lens never bundles a browser. This module either launches an installed Chrome-compatible
//! browser or connects to a caller-provided CDP endpoint, then returns the post-hydration DOM.

use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::cdp::browser_protocol::network::{Headers, SetExtraHttpHeadersParams};
use chromiumoxide::cdp::js_protocol::runtime::EventExceptionThrown;
use chromiumoxide::Page;
use futures::StreamExt;
use serde_json::json;
use std::collections::BTreeMap;
use std::time::Duration;

use crate::error::{SeoError, SeoResult};

const SETTLE_DELAY: Duration = Duration::from_millis(500);
const RUNTIME_ERROR_OBSERVER: &str = r#"
    (() => {
        window.__seolens_runtime_errors = [];
        const record = (error) => {
            const value = error && (error.stack || error.message) ? (error.stack || error.message) : String(error);
            window.__seolens_runtime_errors.push(value);
        };
        window.addEventListener('error', (event) => record(event.error || event.message), true);
        window.addEventListener('unhandledrejection', (event) => record(event.reason), true);
    })();
"#;

/// The browser-observed representation of a single document.
#[derive(Debug, Clone)]
pub struct RenderedDocument {
    /// Serialized DOM after JavaScript execution.
    pub html: String,
    /// Browser URL after any client-side navigation.
    pub final_url: String,
    /// Uncaught errors observed while the document executed.
    pub runtime_errors: Vec<String>,
}

/// A reusable Chrome CDP connection for a crawl.
pub struct JsRenderer {
    browser: Browser,
    handler_task: tokio::task::JoinHandle<()>,
    profile_dir: Option<tempfile::TempDir>,
    owns_browser: bool,
    user_agent: String,
    headers: Vec<(String, String)>,
}

impl JsRenderer {
    /// Connects to a remote CDP endpoint or launches a locally installed browser when omitted.
    pub async fn new(
        chrome_ws: Option<&str>,
        user_agent: String,
        headers: Vec<(String, String)>,
        proxy: Option<&str>,
    ) -> SeoResult<Self> {
        let mut profile_dir = None;
        let owns_browser = chrome_ws.is_none();
        let connection = match chrome_ws {
            Some(endpoint) => Browser::connect(endpoint.to_string()).await,
            None => {
                let profile = tempfile::Builder::new()
                    .prefix("seolens-chrome-")
                    .tempdir()
                    .map_err(SeoError::Io)?;
                let mut builder = BrowserConfig::builder().new_headless_mode().no_sandbox();
                builder = builder.user_data_dir(profile.path());
                if let Some(proxy_url) = proxy {
                    builder = builder.arg(format!("--proxy-server={proxy_url}"));
                }
                let config = builder.build().map_err(|error| {
                    SeoError::Config(format!("Invalid Chrome configuration: {error}"))
                })?;
                let launched = Browser::launch(config).await;
                if launched.is_ok() {
                    profile_dir = Some(profile);
                }
                launched
            }
        }
        .map_err(|error| {
            let source = chrome_ws.unwrap_or("an automatically detected local browser");
            SeoError::Network(format!(
                "Unable to start Chrome CDP rendering through {source}: {error}"
            ))
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
            user_agent,
            headers,
        })
    }

    /// Navigates Chrome to `url`, waits for page load plus a short hydration settle window, and
    /// serializes the resulting DOM.
    pub async fn render(&mut self, url: &str) -> SeoResult<RenderedDocument> {
        let page = self
            .browser
            .new_page("about:blank")
            .await
            .map_err(|error| SeoError::Network(format!("Unable to create Chrome page: {error}")))?;

        let result = self.render_page(&page, url).await;
        // Chrome can discard a target after a renderer crash. The rendering result is still
        // useful in that case, and the browser shutdown path owns final process cleanup.
        let _ = page.close().await;
        result
    }

    async fn render_page(&self, page: &Page, url: &str) -> SeoResult<RenderedDocument> {
        page.set_user_agent(self.user_agent.as_str())
            .await
            .map_err(|error| {
                SeoError::Network(format!("Unable to set Chrome user agent: {error}"))
            })?;

        if !self.headers.is_empty() {
            let headers: BTreeMap<String, String> = self.headers.iter().cloned().collect();
            page.execute(SetExtraHttpHeadersParams::new(Headers::new(json!(headers))))
                .await
                .map_err(|error| {
                    SeoError::Network(format!("Unable to set Chrome request headers: {error}"))
                })?;
        }

        let (error_tx, mut error_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut exceptions = page
            .event_listener::<EventExceptionThrown>()
            .await
            .map_err(|error| {
                SeoError::Internal(format!(
                    "Unable to listen for Chrome runtime errors: {error}"
                ))
            })?;
        let exception_task = tokio::spawn(async move {
            while let Some(event) = exceptions.next().await {
                let _ = error_tx.send(event.exception_details.text.clone());
            }
        });

        page.evaluate_on_new_document(RUNTIME_ERROR_OBSERVER)
            .await
            .map_err(|error| {
                SeoError::Internal(format!("Unable to install runtime error observer: {error}"))
            })?;
        page.goto(url).await.map_err(|error| {
            SeoError::Network(format!("Chrome failed to render {url}: {error}"))
        })?;
        tokio::time::sleep(SETTLE_DELAY).await;

        let html = page.content().await.map_err(|error| {
            SeoError::Internal(format!("Unable to serialize rendered DOM: {error}"))
        })?;
        let final_url = page
            .url()
            .await
            .map_err(|error| SeoError::Internal(format!("Unable to read rendered URL: {error}")))?
            .unwrap_or_else(|| url.to_string());

        let observer_errors: Vec<String> = page
            .evaluate("window.__seolens_runtime_errors || []")
            .await
            .ok()
            .and_then(|value| value.into_value().ok())
            .unwrap_or_default();
        exception_task.abort();

        let mut runtime_errors = Vec::new();
        while let Ok(error) = error_rx.try_recv() {
            runtime_errors.push(error);
        }
        runtime_errors.extend(observer_errors);
        runtime_errors.sort();
        runtime_errors.dedup();

        Ok(RenderedDocument {
            html,
            final_url,
            runtime_errors,
        })
    }

    /// Closes a browser launched by SEO Lens and stops its CDP handler.
    pub async fn shutdown(mut self) -> SeoResult<()> {
        if self.owns_browser {
            self.browser.close().await.map_err(|error| {
                SeoError::Internal(format!("Unable to close Chrome browser: {error}"))
            })?;
        }
        self.handler_task.abort();
        drop(self.profile_dir.take());
        Ok(())
    }
}
