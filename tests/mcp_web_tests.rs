//! Phase 6: agent-mode web tools over MCP (`web_scrape`, `web_map`, `web_crawl`, …).

use blacksparrow::mcp::protocol::{handle_jsonrpc_request, McpContext, Toolset};
use serde_json::{json, Value};
use std::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn site() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/robots.txt"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            "<html><head><title>Home</title></head><body><nav><a href=\"/hours\">Hours</a></nav>\
             <main><h1>Corner bakery</h1><p>Fresh bread every morning from our wood oven.</p>\
             <a href=\"/hours\">Opening hours</a></main></body></html>",
            "text/html",
        ))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/hours"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            "<html><head><title>Hours</title></head><body><main><h1>Opening hours</h1>\
             <p>We are open from 7 am to 3 pm, Tuesday to Sunday.</p>\
             <p>Call us on 555 0100 for large orders.</p></main></body></html>",
            "text/html",
        ))
        .mount(&server)
        .await;
    server
}

fn context(server: &MockServer, toolset: Toolset) -> (McpContext, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let ctx = McpContext::new(Some(dir.path().join("mcp.db")))
        .unwrap()
        .with_allow_local_network(false)
        .with_allowed_hosts(vec![server.address().to_string()])
        .with_toolset(toolset);
    (ctx, dir)
}

async fn rpc(ctx: &McpContext, method: &str, params: Value) -> Value {
    let req = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
    serde_json::from_str(&handle_jsonrpc_request(&req.to_string(), ctx).await).unwrap()
}

async fn call(ctx: &McpContext, name: &str, args: Value) -> (bool, Value) {
    let resp = rpc(
        ctx,
        "tools/call",
        json!({ "name": name, "arguments": args }),
    )
    .await;
    let result = &resp["result"];
    let is_error = result["isError"].as_bool().unwrap_or(false);
    let text = result["content"][0]["text"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let value = serde_json::from_str(&text).unwrap_or(Value::String(text));
    (is_error, value)
}

fn tool_names(list: &Value) -> Vec<String> {
    list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn toolsets_choose_which_tools_are_listed() {
    let server = site().await;
    let (seo, _a) = context(&server, Toolset::Seo);
    let names = tool_names(&rpc(&seo, "tools/list", json!({})).await);
    assert_eq!(names.len(), 8);
    assert!(names.iter().all(|n| n.starts_with("seo_")));

    let (web, _b) = context(&server, Toolset::Web);
    let list = rpc(&web, "tools/list", json!({})).await;
    let names = tool_names(&list);
    for expected in [
        "web_scrape",
        "web_map",
        "web_crawl",
        "web_crawl_status",
        "web_crawl_cancel",
        "web_find",
        "web_extract",
        "web_interact",
    ] {
        assert!(names.contains(&expected.to_string()), "{expected} missing");
    }
    assert!(names.iter().all(|n| n.starts_with("web_")));
    for tool in list["result"]["tools"].as_array().unwrap() {
        let name = tool["name"].as_str().unwrap();
        if ["web_scrape", "web_crawl_status", "web_find", "web_interact"].contains(&name) {
            assert!(
                tool["description"].as_str().unwrap().contains("untrusted"),
                "{name} must warn that page content is untrusted"
            );
        }
        assert_eq!(tool["inputSchema"]["type"], "object");
    }

    let (all, _c) = context(&server, Toolset::All);
    assert_eq!(
        tool_names(&rpc(&all, "tools/list", json!({})).await).len(),
        16
    );
    assert_eq!(Toolset::parse("web"), Some(Toolset::Web));
    assert_eq!(Toolset::parse("nope"), None);
}

#[tokio::test]
async fn web_tools_are_refused_when_not_enabled() {
    let server = site().await;
    let (seo, _dir) = context(&server, Toolset::Seo);
    let (is_error, body) = call(&seo, "web_scrape", json!({ "url": server.uri() })).await;
    assert!(is_error, "{body}");
}

#[tokio::test]
async fn web_scrape_returns_clean_markdown() {
    let server = site().await;
    let (ctx, _dir) = context(&server, Toolset::All);
    let (is_error, doc) = call(
        &ctx,
        "web_scrape",
        json!({ "url": format!("{}/hours", server.uri()) }),
    )
    .await;
    assert!(!is_error, "{doc}");
    assert_eq!(doc["status"], "ok");
    assert!(doc["markdown"]
        .as_str()
        .unwrap()
        .contains("We are open from 7 am to 3 pm"));

    let (is_error, refused) = call(
        &ctx,
        "web_scrape",
        json!({ "url": "http://169.254.169.254/latest/meta-data/" }),
    )
    .await;
    assert!(is_error || refused["status"] == "error", "{refused}");
    assert!(!refused.to_string().contains("ami-id"));
}

#[tokio::test]
async fn web_map_crawl_status_and_find_work_together() {
    let server = site().await;
    let (ctx, _dir) = context(&server, Toolset::All);

    let (is_error, map) = call(
        &ctx,
        "web_map",
        json!({ "url": server.uri(), "sitemap": "skip" }),
    )
    .await;
    assert!(!is_error, "{map}");
    assert!(map["links"]
        .as_array()
        .unwrap()
        .iter()
        .any(|l| l["url"].as_str().unwrap().ends_with("/hours")));

    let (is_error, started) = call(
        &ctx,
        "web_crawl",
        json!({ "url": format!("{}/", server.uri()), "limit": 5 }),
    )
    .await;
    assert!(!is_error, "{started}");
    let id = started["id"].as_str().unwrap().to_string();

    let mut status = Value::Null;
    for _ in 0..100 {
        let (_, s) = call(&ctx, "web_crawl_status", json!({ "id": id })).await;
        if s["state"] == "completed" {
            status = s;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(status["state"], "completed", "crawl never finished");
    assert_eq!(status["pages_done"], 2);

    let (is_error, found) = call(
        &ctx,
        "web_find",
        json!({ "crawl_id": id, "query": "what time do you open" }),
    )
    .await;
    assert!(!is_error, "{found}");
    assert!(found["hits"][0]["text"].as_str().unwrap().contains("7 am"));

    let (is_error, cancelled) = call(&ctx, "web_crawl_cancel", json!({ "id": id })).await;
    assert!(!is_error);
    assert_eq!(
        cancelled["cancelled"], false,
        "a finished crawl cannot be cancelled"
    );
}

#[tokio::test]
async fn web_extract_fills_a_schema() {
    let server = site().await;
    let (ctx, _dir) = context(&server, Toolset::All);
    let (is_error, out) = call(
        &ctx,
        "web_extract",
        json!({
            "url": format!("{}/hours", server.uri()),
            "schema": {
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "phone": { "type": "string", "x-kind": "phone" }
                }
            }
        }),
    )
    .await;
    assert!(!is_error, "{out}");
    assert_eq!(out["results"][0]["data"]["title"], "Opening hours");
    assert!(out["results"][0]["data"]["phone"]
        .as_str()
        .unwrap()
        .contains("555"));
}
