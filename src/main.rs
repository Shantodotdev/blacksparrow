//! # Black Sparrow CLI Entry Point
//!
//! Command-line interface for the `blacksparrow` binary.

use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging subscriber (quiet by default unless RUST_LOG is explicitly provided)
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .init();

    blacksparrow::cli::run().await
}
