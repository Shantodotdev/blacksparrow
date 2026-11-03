//! # Guarded Fetcher
//!
//! The HTTP client of site extraction. Every request target, including each redirect hop, passes
//! the [`AddressPolicy`]; bodies are read in chunks and cut at a byte limit; a slow server is
//! bounded by a per-request timeout. Responses are returned as text (HTML and CSS); binary
//! content is never decoded.

use super::guard::{parse_web_url, AddressPolicy, GuardRefusal, GuardedResolver};
use crate::crawler::waf::detect_waf;
use crate::error::{SeoError, SeoResult};
use reqwest::header::{ACCEPT, ACCEPT_LANGUAGE, CONTENT_TYPE, LOCATION, USER_AGENT};
use reqwest::redirect::Policy;
use std::sync::Arc;
use std::time::Duration;
use url::Url;

/// Redirect hops followed for one fetch.
pub const MAX_REDIRECTS: usize = 5;

/// Why a fetch produced no document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchFailure {
    /// The guard refused the URL or a redirect target.
    Refused(GuardRefusal),
    /// The connection failed or timed out.
    Network(String),
    /// Too many redirects, or a redirect loop.
    Redirects,
}

impl FetchFailure {
    /// Stable snake_case label used in JSON output.
    pub fn label(&self) -> &'static str {
        match self {
            FetchFailure::Refused(refusal) => refusal.as_str(),
            FetchFailure::Network(_) => "network",
            FetchFailure::Redirects => "redirects",
        }
    }
}

/// A fetched text document.
#[derive(Debug, Clone)]
pub struct Document {
    /// The URL after redirects.
    pub final_url: Url,
    /// The HTTP status of the final response.
    pub status: u16,
    /// The `Content-Type` header, lowercased.
    pub content_type: String,
    /// The body decoded as UTF-8 (lossy), cut at the byte limit.
    pub body: String,
    /// Whether the body was cut at the byte limit.
    pub truncated: bool,
    /// Bot-protection provider detected on the response, if any.
    pub waf: Option<&'static str>,
}

impl Document {
    /// Whether the response is an HTML page.
    pub fn is_html(&self) -> bool {
        self.content_type.is_empty()
            || self.content_type.contains("html")
            || self.content_type.contains("xml")
    }
}

/// Fetcher settings.
#[derive(Debug, Clone)]
pub struct FetcherOptions {
    /// User-Agent header.
    pub user_agent: String,
    /// Longest wait for one request, connect included.
    pub timeout: Duration,
    /// Largest body read; the rest is dropped.
    pub max_body_bytes: usize,
    /// Address policy for every target.
    pub policy: AddressPolicy,
}

/// An HTTP client that only reaches addresses its policy permits.
#[derive(Debug, Clone)]
pub struct GuardedFetcher {
    client: reqwest::Client,
    options: Arc<FetcherOptions>,
}

impl GuardedFetcher {
    /// Builds the client.
    ///
    /// # Errors
    ///
    /// Returns [`SeoError::Network`] when the TLS backend cannot be initialised.
    pub fn new(options: FetcherOptions) -> SeoResult<Self> {
        let client = reqwest::Client::builder()
            .redirect(Policy::none())
            .timeout(options.timeout)
            .connect_timeout(Duration::from_secs(8))
            .dns_resolver(Arc::new(GuardedResolver::new(options.policy)))
            .pool_max_idle_per_host(4)
            .gzip(true)
            .brotli(true)
            .deflate(true)
            .build()
            .map_err(|e| SeoError::Network(format!("failed to build HTTP client: {e}")))?;
        Ok(Self {
            client,
            options: Arc::new(options),
        })
    }

    /// The address policy in force.
    pub fn policy(&self) -> AddressPolicy {
        self.options.policy
    }

    /// Fetches `url` as text, following redirects through the guard.
    ///
    /// # Errors
    ///
    /// Returns the [`FetchFailure`] that stopped the fetch. An HTTP error status is not a
    /// failure: the document is returned with its status.
    pub async fn get_text(&self, url: &Url) -> Result<Document, FetchFailure> {
        let mut current = url.clone();
        let mut seen: Vec<String> = Vec::new();
        loop {
            self.options
                .policy
                .check_url(&current)
                .await
                .map_err(FetchFailure::Refused)?;
            let response = self
                .client
                .get(current.clone())
                .header(USER_AGENT, &self.options.user_agent)
                .header(
                    ACCEPT,
                    "text/html,application/xhtml+xml,text/css;q=0.9,*/*;q=0.8",
                )
                .header(ACCEPT_LANGUAGE, "en-US,en;q=0.9,bn;q=0.8")
                .send()
                .await
                .map_err(|e| FetchFailure::Network(short_error(&e)))?;
            let status = response.status();
            if status.is_redirection() {
                let target = response
                    .headers()
                    .get(LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|location| current.join(location).ok());
                if let Some(target) = target {
                    seen.push(current.to_string());
                    if seen.len() > MAX_REDIRECTS || seen.contains(&target.to_string()) {
                        return Err(FetchFailure::Redirects);
                    }
                    current = target;
                    continue;
                }
            }
            let headers = response.headers().clone();
            let content_type = headers
                .get(CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or("")
                .to_ascii_lowercase();
            let (bytes, truncated) = self.read_limited(response).await?;
            let body = String::from_utf8_lossy(&bytes).into_owned();
            let waf = detect_waf(status.as_u16(), &headers, &body);
            return Ok(Document {
                final_url: current,
                status: status.as_u16(),
                content_type,
                body,
                truncated,
                waf,
            });
        }
    }

    /// Parses and fetches a URL string.
    ///
    /// # Errors
    ///
    /// Returns [`FetchFailure::Refused`] for a malformed URL.
    pub async fn get_text_str(&self, raw: &str) -> Result<Document, FetchFailure> {
        let url =
            parse_web_url(raw).map_err(|_| FetchFailure::Refused(GuardRefusal::Unsupported))?;
        self.get_text(&url).await
    }

    async fn read_limited(
        &self,
        mut response: reqwest::Response,
    ) -> Result<(Vec<u8>, bool), FetchFailure> {
        let limit = self.options.max_body_bytes;
        let mut bytes: Vec<u8> = Vec::with_capacity(
            response
                .content_length()
                .map(|len| (len as usize).min(limit))
                .unwrap_or(64 * 1024),
        );
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|e| FetchFailure::Network(short_error(&e)))?
        {
            let room = limit.saturating_sub(bytes.len());
            if chunk.len() >= room {
                bytes.extend_from_slice(&chunk[..room]);
                return Ok((bytes, true));
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok((bytes, false))
    }
}

/// A one-line description of a transport error without the URL (which may be long).
fn short_error(err: &reqwest::Error) -> String {
    if err.is_timeout() {
        "timeout".to_string()
    } else if err.is_connect() {
        "connect failed".to_string()
    } else if err.is_body() || err.is_decode() {
        "body read failed".to_string()
    } else {
        "request failed".to_string()
    }
}
