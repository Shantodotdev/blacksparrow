//! # Black Sparrow CLI Entry Point
//!
//! Command-line interface for the `blacksparrow` binary.

use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging subscriber (quiet by default unless RUST_LOG is explicitly provided).
    // chromiumoxide warns about every CDP event newer than its protocol bindings; those are
    // harmless, so only its errors are shown.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("warn,chromiumoxide=error")),
        )
        .init();

    blacksparrow::cli::run().await
}
