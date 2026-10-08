//! # Black Sparrow CLI Entry Point
//!
//! Command-line interface for the `blacksparrow` binary.

use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Logs go to stderr so stdout stays clean for Markdown, NDJSON and MCP frames. Quiet by
    // default unless RUST_LOG is set; the HTTP server's one-line request log stays on.
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("warn,blacksparrow::serve=info")),
        )
        .init();

    blacksparrow::cli::run().await
}
