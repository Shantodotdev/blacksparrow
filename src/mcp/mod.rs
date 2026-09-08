//! # Model Context Protocol (MCP) Server
//!
//! Exposes SEO Lens inspection tools and crawl session resources to AI agents
//! via standard JSON-RPC 2.0 over `stdio`.

pub mod formatter;
pub mod protocol;
pub mod resources;
pub mod server;
pub mod tools;
pub mod types;

pub use protocol::{handle_jsonrpc_request, McpContext};
pub use server::{run_mcp_server, run_mcp_server_io};
pub use tools::get_tool_definitions;
pub use types::*;
