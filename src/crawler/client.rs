//! # Asynchronous HTTP Client Wrapper
//!
//! High-performance asynchronous HTTP/2 client wrapper around `reqwest` with
//! custom redirect policy handling, redirect loop detection, TTFB latency measurement,
//! and transport error classification.

use crate::core::url::resolve_relative;
use crate::crawler::waf::detect_waf;
use crate::error::{SeoError, SeoResult};
use compact_str::CompactString;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, CONTENT_TYPE, LOCATION, USER_AGENT};
use reqwest::redirect::Policy;
use std::str::FromStr;
use std::time::{Duration, Instant};

/// Options for configuring an [`HttpClient`] instance.
#[derive(Debug, Clone)]
pub struct FetchOptions {
    /// User-Agent string sent in request headers.
    pub user_agent: String,
    /// Total request timeout.
    pub timeout: Duration,
    /// TCP connection timeout.
    pub connect_timeout: Duration,
    /// Maximum number of redirect hops to follow (default 10).
    pub max_redirects: usize,
    /// Custom headers to attach to every outgoing request.
    pub custom_headers: Vec<(String, String)>,
    /// Optional proxy URL (HTTP, HTTPS, SOCKS5).
    pub proxy: Option<String>,
}

impl Default for FetchOptions {
    fn default() -> Self {
        Self {
            user_agent: "SEOLens/1.0".to_string(),
            timeout: Duration::from_secs(30),
            connect_timeout: Duration::from_secs(10),
            max_redirects: 10,
            custom_headers: Vec::new(),
            proxy: None,
        }
    }
}

/// The result of an HTTP page fetch operation.
#[derive(Debug, Clone)]
pub struct FetchResult {
    /// Initial request URL before any redirects.
    pub url: String,
    /// Final destination URL after following redirect hops.
    pub final_url: String,
    /// HTTP status code (e.g. 200, 301, 404, 500).
    pub status_code: u16,
    /// Raw HTTP response headers.
    pub headers: HeaderMap,
    /// Content-Type header value (e.g. "text/html; charset=utf-8").
    pub content_type: CompactString,
    /// Response payload text decoded as UTF-8 (lossy).
    pub body: String,
    /// Raw response payload bytes.
    pub body_bytes: Vec<u8>,
    /// Payload size in bytes.
    pub size_bytes: u32,
    /// Time-to-first-byte (TTFB) in milliseconds.
    pub ttfb_ms: u32,
    /// Chronological list of intermediate redirect URLs.
    pub redirect_chain: Vec<String>,
    /// WAF / Bot challenge detected on the response, if any.
    pub waf_detected: Option<&'static str>,
}

/// Asynchronous HTTP client configured for technical SEO crawling.
#[derive(Debug, Clone)]
pub struct HttpClient {
    client: reqwest::Client,
    options: FetchOptions,
}

impl HttpClient {
    /// Creates a new `HttpClient` with the given configuration options.
    ///
    /// # Errors
    ///
    /// Returns [`SeoError::Config`] or [`SeoError::Network`] if client initialization fails.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use seo_lens::crawler::client::{HttpClient, FetchOptions};
    ///
    /// let client = HttpClient::new(FetchOptions::default()).unwrap();
    /// ```
    pub fn new(options: FetchOptions) -> SeoResult<Self> {
        let mut builder = reqwest::Client::builder()
            .timeout(options.timeout)
            .connect_timeout(options.connect_timeout)
            .redirect(Policy::none()) // We manage redirects manually to track hops and chains
            .gzip(true)
            .brotli(true)
            .deflate(true);

        if let Some(ref proxy_url) = options.proxy {
            let proxy = reqwest::Proxy::all(proxy_url)
                .map_err(|e| SeoError::Config(format!("Invalid proxy configuration: {e}")))?;
            builder = builder.proxy(proxy);
        }

        let client = builder
            .build()
            .map_err(|e| SeoError::Network(format!("Failed to build HTTP client: {e}")))?;

        Ok(Self { client, options })
    }

    /// Fetches a URL, following HTTP redirects up to `max_redirects` and recording telemetry.
    ///
    /// # Errors
    ///
    /// Returns [`SeoError::Network`] on DNS failure, connection refused, timeout, or redirect loops.
    pub async fn fetch(&self, url: &str) -> SeoResult<FetchResult> {
        let mut current_url = url.to_string();
        let mut redirect_chain = Vec::new();
        let initial_start = Instant::now();
        let mut ttfb_ms = 0u32;

        loop {
            let mut req = self.client.get(&current_url);

            // Add User-Agent
            req = req.header(USER_AGENT, &self.options.user_agent);

            // Add custom headers
            for (key, val) in &self.options.custom_headers {
                if let (Ok(name), Ok(value)) =
                    (HeaderName::from_str(key), HeaderValue::from_str(val))
                {
                    req = req.header(name, value);
                }
            }

            let response = req.send().await.map_err(|err| {
                if err.is_timeout() {
                    SeoError::Network(format!("Request timeout fetching {current_url}: {err}"))
                } else if err.is_connect() {
                    SeoError::Network(format!("Connection failure fetching {current_url}: {err}"))
                } else {
                    SeoError::Network(format!(
                        "HTTP transport error fetching {current_url}: {err}"
                    ))
                }
            })?;

            if ttfb_ms == 0 {
                ttfb_ms = initial_start.elapsed().as_millis() as u32;
            }

            let status = response.status();
            let status_code = status.as_u16();
            let headers = response.headers().clone();

            // Handle redirect codes (301, 302, 303, 307, 308)
            if status.is_redirection() {
                if let Some(loc_header) = headers.get(LOCATION) {
                    if let Ok(loc_str) = loc_header.to_str() {
                        let target_url = resolve_relative(&current_url, loc_str)?;

                        // Check for circular redirect
                        if redirect_chain.contains(&current_url) || current_url == target_url {
                            return Err(SeoError::Network(format!(
                                "Circular redirect detected between {current_url} and {target_url}"
                            )));
                        }

                        redirect_chain.push(current_url);

                        if redirect_chain.len() > self.options.max_redirects {
                            return Err(SeoError::Network(format!(
                                "Exceeded maximum redirect hops ({}) starting from {url}",
                                self.options.max_redirects
                            )));
                        }

                        current_url = target_url;
                        continue;
                    }
                }
            }

            // Read response payload
            let content_type = headers
                .get(CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(CompactString::new)
                .unwrap_or_else(|| CompactString::new(""));

            let body_bytes = response
                .bytes()
                .await
                .map_err(|e| SeoError::Network(format!("Failed to read response body: {e}")))?
                .to_vec();

            let size_bytes = body_bytes.len() as u32;
            let body = String::from_utf8_lossy(&body_bytes).to_string();

            let waf_detected = detect_waf(status_code, &headers, &body);

            return Ok(FetchResult {
                url: url.to_string(),
                final_url: current_url,
                status_code,
                headers,
                content_type,
                body,
                body_bytes,
                size_bytes,
                ttfb_ms,
                redirect_chain,
                waf_detected,
            });
        }
    }
}
