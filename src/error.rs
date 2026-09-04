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
