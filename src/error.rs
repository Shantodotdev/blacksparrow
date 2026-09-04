use thiserror::Error;

/// Core error type for all SEO Lens operations.
#[derive(Error, Debug)]
pub enum SeoError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("URL parsing error: {0}")]
    Url(String),

    #[error("Network error: {0}")]
    Network(String),

    #[error("Internal engine error: {0}")]
    Internal(String),
}

/// Specialized Result type for SEO Lens operations.
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
