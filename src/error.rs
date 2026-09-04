//! # Error Handling & Result Types
//!
//! Unified error taxonomy for `SEO Lens` using [`thiserror`].
//!
//! ## Zero-Panic Principle
//!
//! In compliance with core architectural guidelines, library code in `src/` never panics
//! (`unwrap()` and `expect()` are forbidden). All fallible operations return [`SeoResult<T>`]
//! wrapping an explicit [`SeoError`].

use thiserror::Error;

/// Core error taxonomy for all SEO Lens operations.
///
/// Classifies failures across I/O, serialization, configuration, URL parsing,
/// HTTP network transport, and internal parser engine states.
///
/// # Examples
///
/// ```rust
/// use seo_lens::error::SeoError;
///
/// let err = SeoError::Url("Unsupported scheme: ftp".to_string());
/// assert_eq!(err.to_string(), "URL parsing error: Unsupported scheme: ftp");
/// ```
#[derive(Error, Debug)]
pub enum SeoError {
    /// File system or transport I/O failure.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON or data model serialization / deserialization error.
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Crawl parameter validation failure (e.g. invalid concurrency or empty user-agent).
    #[error("Configuration error: {0}")]
    Config(String),

    /// URL parsing error, non-HTTP scheme, or RFC 3986 normalization failure.
    #[error("URL parsing error: {0}")]
    Url(String),

    /// HTTP network transport failure (DNS resolution, connection timeout, SSL handshake).
    #[error("Network error: {0}")]
    Network(String),

    /// Unexpected internal engine state or parser rewriter failure.
    #[error("Internal engine error: {0}")]
    Internal(String),
}

/// Specialized Result type for SEO Lens operations.
///
/// Convenient alias equivalent to `Result<T, SeoError>`.
pub type SeoResult<T> = Result<T, SeoError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_formatting() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let seo_err = SeoError::from(io_err);
        assert!(seo_err.to_string().contains("I/O error"));

        let config_err = SeoError::Config("invalid max_pages".to_string());
        assert_eq!(
            config_err.to_string(),
            "Configuration error: invalid max_pages"
        );

        let url_err = SeoError::Url("missing scheme".to_string());
        assert_eq!(url_err.to_string(), "URL parsing error: missing scheme");

        let net_err = SeoError::Network("connection reset".to_string());
        assert_eq!(net_err.to_string(), "Network error: connection reset");

        let internal_err = SeoError::Internal("unexpected state".to_string());
        assert_eq!(
            internal_err.to_string(),
            "Internal engine error: unexpected state"
        );
    }
}
