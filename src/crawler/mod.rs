//! # Crawler Module
//!
//! Asynchronous network fetching engine, AIMD adaptive politeness controller,
//! and WAF / bot challenge fingerprint probes.
//!
//! ## Modules
//!
//! - [`client`]: Asynchronous HTTP client wrapper around `reqwest` with manual redirect tracking and TTFB timing.
//! - [`aimd`]: Additive-Increase/Multiplicative-Decrease congestion controller protecting origin servers.
//! - [`waf`]: Bot challenge fingerprint scanner detecting Cloudflare, Akamai, DataDome, and Imperva screens.

pub mod aimd;
pub mod client;
pub mod waf;

pub use aimd::AimdController;
pub use client::{FetchOptions, FetchResult, HttpClient};
pub use waf::detect_waf;
