//! # JSON-RPC 2.0 MCP Protocol Dispatcher
//!
//! Handles request routing, protocol handshake, tool invocations, and resource queries
//! conforming to the Model Context Protocol (2024-11-05).

use crate::error::SeoResult;
use crate::mcp::resources::{get_resource_definitions, read_resource};
use crate::mcp::tools::{execute_tool, get_tool_definitions};
use crate::mcp::types::{
    CallToolResult, JsonRpcRequest, JsonRpcResponse, INVALID_PARAMS, METHOD_NOT_FOUND, PARSE_ERROR,
};
use crate::storage::{default_db_path, Database};
use serde_json::json;
use std::path::PathBuf;

/// Shared runtime context for the MCP server.
#[derive(Debug, Clone)]
pub struct McpContext {
    /// SQLite database handle for state inspection and crawl records.
    pub db: Database,
}

impl McpContext {
    /// Creates a new MCP context with an optional database path.
    ///
    /// # Errors
    ///
    /// Returns [`SeoError::Storage`] if the SQLite database cannot be opened or initialized.
    pub fn new(db_path: Option<PathBuf>) -> SeoResult<Self> {
        let path = db_path.unwrap_or_else(default_db_path);
        let db = Database::open(&path)?;
        Ok(Self { db })
    }

    /// Creates a context with a pre-configured database instance.
    pub fn with_database(db: Database) -> Self {
        Self { db }
    }
}

/// Dispatches a single JSON-RPC 2.0 request string and returns the serialized JSON-RPC response.
pub async fn handle_jsonrpc_request(raw_json: &str, ctx: &McpContext) -> String {
    let request: JsonRpcRequest = match serde_json::from_str(raw_json) {
        Ok(req) => req,
        Err(e) => {
            let err_resp = JsonRpcResponse::error(
                None,
                PARSE_ERROR,
                format!("Failed to parse JSON-RPC request: {e}"),
                None,
            );
            return serde_json::to_string(&err_resp).unwrap_or_else(|_| "{}".to_string());
        }
    };

    let id = request.id.clone();
    let method = request.method.as_str();

    let response = match method {
        "initialize" => {
            let server_version = env!("CARGO_PKG_VERSION");
            JsonRpcResponse::success(
                id,
                json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {
                        "tools": {},
                        "resources": {}
                    },
                    "serverInfo": {
                        "name": "seolens",
                        "version": server_version
                    }
                }),
            )
        }
        "notifications/initialized" => {
            if id.is_some() {
                JsonRpcResponse::success(id, json!({}))
            } else {
                return String::new();
            }
        }
        "ping" => JsonRpcResponse::success(id, json!({})),
        "tools/list" => {
            let tools = get_tool_definitions();
            JsonRpcResponse::success(id, json!({ "tools": tools }))
        }
        "tools/call" => {
            let params = request.params.unwrap_or_else(|| json!({}));
            let tool_name = match params["name"].as_str() {
                Some(n) => n,
                None => {
                    return serde_json::to_string(&JsonRpcResponse::error(
                        id,
                        INVALID_PARAMS,
                        "Missing required parameter 'name' in tools/call",
                        None,
                    ))
                    .unwrap_or_else(|_| "{}".to_string());
                }
            };

            let tool_args = params.get("arguments");
            match execute_tool(tool_name, tool_args, &ctx.db).await {
                Ok(res) => {
                    let val = serde_json::to_value(&res).unwrap_or_else(|_| json!({}));
                    JsonRpcResponse::success(id, val)
                }
                Err(e) => {
                    let err_call = CallToolResult::error(format!("Tool error: {e}"));
                    let val = serde_json::to_value(&err_call).unwrap_or_else(|_| json!({}));
                    JsonRpcResponse::success(id, val)
                }
            }
        }
        "resources/list" => {
            let resources = get_resource_definitions(&ctx.db);
            JsonRpcResponse::success(id, json!({ "resources": resources }))
        }
        "resources/read" => {
            let params = request.params.unwrap_or_else(|| json!({}));
            let uri = match params["uri"].as_str() {
                Some(u) => u,
                None => {
                    return serde_json::to_string(&JsonRpcResponse::error(
                        id,
                        INVALID_PARAMS,
                        "Missing required parameter 'uri' in resources/read",
                        None,
                    ))
                    .unwrap_or_else(|_| "{}".to_string());
                }
            };

            match read_resource(uri, &ctx.db) {
                Ok(res) => {
                    let val = serde_json::to_value(&res).unwrap_or_else(|_| json!({}));
                    JsonRpcResponse::success(id, val)
                }
                Err(e) => JsonRpcResponse::error(
                    id,
                    -32002, // Resource not found or error
                    format!("Failed to read resource '{uri}': {e}"),
                    None,
                ),
            }
        }
        _ => JsonRpcResponse::error(
            id,
            METHOD_NOT_FOUND,
            format!("Method '{method}' not found"),
            None,
        ),
    };

    serde_json::to_string(&response).unwrap_or_else(|_| "{}".to_string())
}
