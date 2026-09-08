//! # MCP Stdio Server Runner
//!
//! Provides the asynchronous stdio JSON-RPC 2.0 loop reading from `stdin`
//! and writing responses to `stdout`.

use crate::error::SeoResult;
use crate::mcp::protocol::{handle_jsonrpc_request, McpContext};
use std::path::PathBuf;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

/// Runs the MCP JSON-RPC 2.0 server over generic asynchronous reader and writer streams.
pub async fn run_mcp_server_io<R, W>(
    reader: R,
    mut writer: W,
    db_path: Option<PathBuf>,
) -> SeoResult<()>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let ctx = McpContext::new(db_path)?;
    let mut lines = reader.lines();

    while let Some(line) = lines.next_line().await? {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let response = handle_jsonrpc_request(trimmed, &ctx).await;
        if !response.is_empty() {
            writer.write_all(response.as_bytes()).await?;
            writer.write_all(b"\n").await?;
            writer.flush().await?;
        }
    }

    Ok(())
}

/// Runs the MCP server over standard input (`stdin`) and standard output (`stdout`).
///
/// Log messages and diagnostics are sent to `stderr` so as not to corrupt JSON-RPC frames on `stdout`.
pub async fn run_mcp_server(db_path: Option<PathBuf>) -> SeoResult<()> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    let reader = tokio::io::BufReader::new(stdin);
    run_mcp_server_io(reader, stdout, db_path).await
}
