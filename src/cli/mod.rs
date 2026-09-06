//! # Command-Line Interface Module
//!
//! Exposes the CLI parser, subcommand structures, and execution orchestration for the `seolens` binary.

pub mod args;
pub mod commands;

pub use args::{AuditArgs, Cli, Commands, InspectArgs, McpArgs, ReportArgs};
pub use commands::execute;

use crate::report::print_cli_help;
use clap::Parser;

/// Parses command-line arguments and dispatches execution to the corresponding command handler.
pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    // Intercept top-level help or no-arguments invocation to present the cyberpunk home/help TUI
    if args.len() <= 1
        || (args.len() == 2 && (args[1] == "--help" || args[1] == "-h" || args[1] == "help"))
    {
        print_cli_help();
        return Ok(());
    }

    let cli = Cli::parse();
    execute(cli).await
}
