//! # Command-Line Interface Module
//!
//! Exposes the CLI parser, subcommand structures, and execution orchestration for the `seolens` binary.

pub mod args;
pub mod commands;

pub use args::{AuditArgs, Cli, Commands, InspectArgs, McpArgs, ReportArgs};
pub use commands::execute;

use clap::Parser;

/// Parses command-line arguments and dispatches execution to the corresponding command handler.
pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    execute(cli).await
}
