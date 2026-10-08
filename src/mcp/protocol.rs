//! # JSON-RPC 2.0 MCP Protocol Dispatcher
//!
//! Handles request routing, protocol handshake, tool invocations, and resource queries
//! conforming to the Model Context Protocol (2024-11-05).

use crate::error::SeoResult;
use crate::extract::scrape::ScraperConfig;
use crate::mcp::resources::{get_resource_definitions, read_resource};
use crate::mcp::tools::{execute_tool, get_tool_definitions};
use crate::mcp::types::{
    CallToolResult, JsonRpcRequest, JsonRpcResponse, INVALID_PARAMS, METHOD_NOT_FOUND, PARSE_ERROR,
};
use crate::mcp::web_tools::{execute_web_tool, web_tool_definitions, WebTools};
use crate::storage::{default_db_path, Database};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::OnceCell;

/// Which tool families the server lists and accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Toolset {
    /// The 8 `seo_*` audit tools.
    #[default]
    Seo,
    /// The `web_*` agent tools (scrape, map, crawl, find, extract, interact).
    Web,
    /// Both families.
    All,
}

impl Toolset {
    /// Parses `seo`, `web` or `all`.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "seo" => Some(Self::Seo),
            "web" => Some(Self::Web),
            "all" => Some(Self::All),
            _ => None,
        }
    }

    fn seo(self) -> bool {
        matches!(self, Self::Seo | Self::All)
    }

    fn web(self) -> bool {
        matches!(self, Self::Web | Self::All)
    }
}

/// Shared runtime context for the MCP server.
#[derive(Debug, Clone)]
pub struct McpContext {
    /// SQLite database handle for state inspection and crawl records.
    pub db: Database,
    /// Allow fetching from local/private network addresses (default: false in production).
    pub allow_local_network: bool,
    /// List of specifically allowed private hostnames or IP:port destinations.
    pub allowed_hosts: Vec<String>,
    /// Tool families exposed to the client.
    pub toolset: Toolset,
    /// Base scraper settings for the `web_*` tools (network permissions and the database
    /// path come from this context).
    pub scraper_config: ScraperConfig,
    web: Arc<OnceCell<Arc<WebTools>>>,
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
        let allow_local = std::env::var("BLACKSPARROW_MCP_ALLOW_LOCAL")
            .map(|v| v == "1" || v == "true")
            .unwrap_or_else(|_| std::env::var("CARGO_MANIFEST_DIR").is_ok() || cfg!(test));
        Ok(Self::build(db, allow_local))
    }

    /// Creates a context with a pre-configured database instance.
    pub fn with_database(db: Database) -> Self {
        let allow_local = std::env::var("BLACKSPARROW_MCP_ALLOW_LOCAL")
            .map(|v| v == "1" || v == "true")
            .unwrap_or_else(|_| std::env::var("CARGO_MANIFEST_DIR").is_ok() || cfg!(test));
        Self::build(db, allow_local)
    }

    fn build(db: Database, allow_local_network: bool) -> Self {
        Self {
            db,
            allow_local_network,
            allowed_hosts: Vec::new(),
            toolset: Toolset::default(),
            scraper_config: ScraperConfig::default(),
            web: Arc::new(OnceCell::new()),
        }
    }

    /// Chooses the tool families exposed to the client.
    pub fn with_toolset(mut self, toolset: Toolset) -> Self {
        self.toolset = toolset;
        self
    }

    /// Sets the base scraper settings for the `web_*` tools (user agent, headers, Chrome
    /// endpoint, robots.txt).
    pub fn with_scraper_config(mut self, config: ScraperConfig) -> Self {
        self.scraper_config = config;
        self
    }

    /// The `web_*` tool state, created on first use so SEO-only sessions never build it.
    async fn web_tools(&self) -> SeoResult<Arc<WebTools>> {
        self.web
            .get_or_try_init(|| async {
                let mut config = self.scraper_config.clone();
                config.allow_all_private_ips = self.allow_local_network;
                config
                    .allowed_private_hosts
                    .extend(self.allowed_hosts.iter().cloned());
                config.db_path = Some(self.db.path().to_path_buf());
                WebTools::new(config).map(Arc::new)
            })
            .await
            .cloned()
    }

    /// Sets whether local and private network addresses can be fetched by default.
    pub fn with_allow_local_network(mut self, allow: bool) -> Self {
        self.allow_local_network = allow;
        self
    }

    /// Sets the list of specifically allowed private hostnames or IP:port destinations.
    pub fn with_allowed_hosts(mut self, hosts: Vec<String>) -> Self {
        self.allowed_hosts = hosts;
        self
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
                        "name": crate::core::branding::MCP_SERVER_NAME,
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
            let mut tools = Vec::new();
            if ctx.toolset.seo() {
                tools.extend(get_tool_definitions());
            }
            if ctx.toolset.web() {
                tools.extend(web_tool_definitions());
            }
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
            let outcome = if tool_name.starts_with("web_") {
                if ctx.toolset.web() {
                    match ctx.web_tools().await {
                        Ok(web) => execute_web_tool(tool_name, tool_args, &web).await,
                        Err(e) => Err(e),
                    }
                } else {
                    Ok(CallToolResult::error(format!(
                        "Tool '{tool_name}' is not enabled; start the server with --tools web or --tools all"
                    )))
                }
            } else if ctx.toolset.seo() {
                execute_tool(
                    tool_name,
                    tool_args,
                    &ctx.db,
                    ctx.allow_local_network,
                    &ctx.allowed_hosts,
                )
                .await
            } else {
                Ok(CallToolResult::error(format!(
                    "Tool '{tool_name}' is not enabled; start the server with --tools seo or --tools all"
                )))
            };
            match outcome {
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
