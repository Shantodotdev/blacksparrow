//! Phase 6: the HTTP API (`blacksparrow serve`), Firecrawl-compatible request and response
//! shapes, API keys, rate limits and job caps.
#![cfg(feature = "serve")]

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use axum::Router;
use blacksparrow::extract::scrape::{Scraper, ScraperConfig};
use blacksparrow::serve::{check_bind, router, AppState, ServeConfig};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::time::Duration;
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const KEY: &str = "test-key-123";

const ARTICLE: &str = r#"<!doctype html><html lang="en"><head><title>Tide pools</title>
<meta name="description" content="A guide to tide pools."></head><body>
<nav><a href="/">Home</a> <a href="/about">About</a></nav>
<main><h1>Tide pools</h1>
<p>Tide pools form on rocky shores when the sea pulls back at low tide.</p>
<p>Sea stars, anemones and small crabs live in them. Visit <a href="/guide">the guide</a>.</p>
<p class="price">Entry costs $12.50 per adult.</p>
</main></body></html>"#;

async fn site() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/robots.txt"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    for route in ["/", "/article"] {
        Mock::given(method("GET"))
            .and(path(route))
            .respond_with(ResponseTemplate::new(200).set_body_raw(ARTICLE, "text/html"))
            .mount(&server)
            .await;
    }
    for route in ["/guide", "/about"] {
        Mock::given(method("GET"))
            .and(path(route))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                format!(
                    "<html><head><title>{route}</title></head><body><main><h1>Page {route}</h1>\
                     <p>Some text about {route} that is long enough to keep.</p>\
                     <a href=\"/\">Home</a></main></body></html>"
                ),
                "text/html",
            ))
            .mount(&server)
            .await;
    }
    server
}

fn app_with(server: &MockServer, config: ServeConfig) -> Router {
    let scraper = Scraper::new(ScraperConfig {
        allowed_private_hosts: vec![server.address().to_string()],
        ..Default::default()
    })
    .unwrap();
    router(AppState::new(scraper, config))
}

fn app(server: &MockServer) -> Router {
    app_with(
        server,
        ServeConfig {
            api_keys: vec![KEY.into()],
            ..Default::default()
        },
    )
}

async fn call(app: &Router, method: &str, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
    call_with_key(app, method, uri, body, Some(KEY)).await
}

async fn call_with_key(
    app: &Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
    key: Option<&str>,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "api.test");
    if let Some(key) = key {
        req = req.header("authorization", format!("Bearer {key}"));
    }
    let req = match body {
        Some(b) => req
            .header("content-type", "application/json")
            .body(Body::from(b.to_string())),
        None => req.body(Body::empty()),
    }
    .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = to_bytes(res.into_body(), 64 * 1024 * 1024).await.unwrap();
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

#[tokio::test]
async fn health_needs_no_key() {
    let server = site().await;
    let (status, body) = call_with_key(&app(&server), "GET", "/health", None, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");
}

#[tokio::test]
async fn requests_need_a_valid_api_key() {
    let server = site().await;
    let app = app(&server);
    let body = json!({ "url": format!("{}/article", server.uri()) });
    let (status, err) = call_with_key(&app, "POST", "/v1/scrape", Some(body.clone()), None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(err["success"], false);
    let (status, _) = call_with_key(
        &app,
        "POST",
        "/v1/scrape",
        Some(body.clone()),
        Some("wrong"),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = call(&app, "POST", "/v1/scrape", Some(body)).await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn scrape_returns_the_firecrawl_shape() {
    let server = site().await;
    let url = format!("{}/article", server.uri());
    let (status, body) = call(
        &app(&server),
        "POST",
        "/v1/scrape",
        Some(json!({
            "url": url,
            "formats": ["markdown", "links", "rawHtml", {"type": "json", "prompt": "ignored"}],
            "onlyMainContent": true,
            "timeout": 20000
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["success"], true);
    let data = &body["data"];
    assert!(data["markdown"].as_str().unwrap().contains("# Tide pools"));
    assert!(!data["markdown"].as_str().unwrap().contains("About"));
    assert!(data["rawHtml"].as_str().unwrap().contains("<nav>"));
    let links: Vec<&str> = data["links"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l.as_str().unwrap())
        .collect();
    assert!(links.contains(&format!("{}/guide", server.uri()).as_str()));
    let meta = &data["metadata"];
    assert_eq!(meta["title"], "Tide pools");
    assert_eq!(meta["description"], "A guide to tide pools.");
    assert_eq!(meta["language"], "en");
    assert_eq!(meta["sourceURL"], url);
    assert_eq!(meta["statusCode"], 200);
    assert!(data.get("html").is_none(), "html was not requested");
}

#[tokio::test]
async fn private_and_metadata_addresses_are_refused() {
    let server = site().await;
    let app = app(&server);
    for target in [
        "http://169.254.169.254/latest/meta-data/",
        "http://127.0.0.1:9/admin",
        "http://10.0.0.5/",
    ] {
        let (status, body) = call(&app, "POST", "/v1/scrape", Some(json!({ "url": target }))).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{target}: {body}");
        assert_eq!(body["success"], false);
        let (status, _) = call(&app, "POST", "/v1/crawl", Some(json!({ "url": target }))).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{target}");
    }
    let (status, body) = call(
        &app,
        "POST",
        "/v1/scrape",
        Some(json!({ "url": "ftp://x" })),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
}

#[tokio::test]
async fn bad_bodies_are_rejected_with_json_errors() {
    let server = site().await;
    let app = app(&server);
    let (status, body) = call(&app, "POST", "/v1/scrape", Some(json!({ "nope": 1 }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["success"], false);
    assert!(body["error"].as_str().unwrap().contains("url"));

    let small = app_with(
        &server,
        ServeConfig {
            api_keys: vec![KEY.into()],
            max_body_bytes: 64,
            ..Default::default()
        },
    );
    let (status, _) = call(
        &small,
        "POST",
        "/v1/scrape",
        Some(json!({ "url": "https://example.com/", "padding": "x".repeat(500) })),
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn map_returns_strings_on_v1_and_objects_on_v2() {
    let server = site().await;
    let app = app(&server);
    let body = json!({ "url": server.uri(), "ignoreSitemap": true });
    let (status, v1) = call(&app, "POST", "/v1/map", Some(body.clone())).await;
    assert_eq!(status, StatusCode::OK, "{v1}");
    let links = v1["links"].as_array().unwrap();
    assert!(links
        .iter()
        .any(|l| l.as_str() == Some(&format!("{}/guide", server.uri()))));

    let (status, v2) = call(&app, "POST", "/v2/map", Some(body)).await;
    assert_eq!(status, StatusCode::OK);
    let first = &v2["links"].as_array().unwrap()[0];
    assert!(first["url"].is_string());
}

#[tokio::test]
async fn crawl_jobs_start_report_progress_and_page_results() {
    let server = site().await;
    let app = app(&server);
    let (status, started) = call(
        &app,
        "POST",
        "/v1/crawl",
        Some(json!({
            "url": format!("{}/", server.uri()),
            "limit": 10,
            "scrapeOptions": { "formats": ["markdown"] }
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{started}");
    assert_eq!(started["success"], true);
    let id = started["id"].as_str().unwrap().to_string();
    assert_eq!(
        started["url"],
        format!("http://api.test/v1/crawl/{id}"),
        "status URL uses the request host"
    );

    let mut done = Value::Null;
    for _ in 0..100 {
        let (status, body) = call(&app, "GET", &format!("/v1/crawl/{id}"), None).await;
        assert_eq!(status, StatusCode::OK);
        if body["status"] == "completed" {
            done = body;
            break;
        }
        assert_eq!(body["status"], "scraping");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(done["status"], "completed", "crawl never finished");
    assert_eq!(done["completed"], 3);
    assert_eq!(done["total"], 3);
    let data = done["data"].as_array().unwrap();
    assert_eq!(data.len(), 3);
    assert!(data
        .iter()
        .all(|d| d["markdown"].is_string() && d["metadata"]["sourceURL"].is_string()));

    let (_, paged) = call(&app, "GET", &format!("/v1/crawl/{id}?limit=1"), None).await;
    assert_eq!(paged["data"].as_array().unwrap().len(), 1);
    assert_eq!(
        paged["next"],
        format!("http://api.test/v1/crawl/{id}?skip=1&limit=1")
    );
    let (_, last) = call(&app, "GET", &format!("/v1/crawl/{id}?skip=2&limit=1"), None).await;
    assert!(last["next"].is_null());

    let (status, _) = call(&app, "GET", "/v1/crawl/crawl_unknown", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(&app, "DELETE", "/v1/crawl/crawl_unknown", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn crawl_caps_limit_pages_and_concurrent_jobs() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/robots.txt"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let links: String = (0..20)
        .map(|i| format!("<a href=\"/p{i}\">Page {i}</a>"))
        .collect();
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(150))
                .set_body_raw(
                    format!("<html><body><main><h1>Slow</h1><p>Slow page text.</p>{links}</main></body></html>"),
                    "text/html",
                ),
        )
        .mount(&server)
        .await;
    let app = app_with(
        &server,
        ServeConfig {
            api_keys: vec![KEY.into()],
            max_crawl_pages: 3,
            max_concurrent_crawls: 1,
            ..Default::default()
        },
    );
    let body = json!({ "url": format!("{}/", server.uri()), "limit": 50, "ignoreSitemap": true });
    let (status, first) = call(&app, "POST", "/v1/crawl", Some(body.clone())).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    let (status, busy) = call(&app, "POST", "/v1/crawl", Some(body)).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS, "{busy}");

    let id = first["id"].as_str().unwrap();
    let (status, cancelled) = call(&app, "DELETE", &format!("/v1/crawl/{id}"), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cancelled["status"], "cancelled");

    let mut finished = Value::Null;
    for _ in 0..100 {
        let (_, body) = call(&app, "GET", &format!("/v1/crawl/{id}"), None).await;
        if body["status"] != "scraping" {
            finished = body;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(finished["status"], "cancelled");
    assert!(finished["completed"].as_u64().unwrap() <= 3);
}

#[tokio::test]
async fn each_key_is_rate_limited() {
    let server = site().await;
    let app = app_with(
        &server,
        ServeConfig {
            api_keys: vec![KEY.into(), "other-key".into()],
            requests_per_minute: 2,
            ..Default::default()
        },
    );
    let body = json!({ "url": format!("{}/article", server.uri()), "formats": ["links"] });
    for _ in 0..2 {
        let (status, _) = call(&app, "POST", "/v1/scrape", Some(body.clone())).await;
        assert_eq!(status, StatusCode::OK);
    }
    let (status, limited) = call(&app, "POST", "/v1/scrape", Some(body.clone())).await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(limited["success"], false);
    let (status, _) =
        call_with_key(&app, "POST", "/v1/scrape", Some(body), Some("other-key")).await;
    assert_eq!(status, StatusCode::OK, "another key has its own budget");
}

#[tokio::test]
async fn find_and_extract_endpoints_work_on_one_page() {
    let server = site().await;
    let app = app(&server);
    let url = format!("{}/article", server.uri());

    let (status, found) = call(
        &app,
        "POST",
        "/v1/find",
        Some(json!({ "url": url, "query": "what lives in tide pools" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{found}");
    assert!(found["data"]["hits"][0]["text"]
        .as_str()
        .unwrap()
        .contains("anemones"));

    let (status, extracted) = call(
        &app,
        "POST",
        "/v1/extract",
        Some(json!({
            "urls": [url],
            "schema": {
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "price": { "type": "number", "x-kind": "price" }
                }
            }
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{extracted}");
    assert_eq!(extracted["success"], true);
    assert_eq!(extracted["data"]["title"], "Tide pools");
    assert_eq!(extracted["data"]["price"], 12.5);
    assert!(extracted["results"][0]["fields"].is_object());

    let (status, err) = call(&app, "POST", "/v1/find", Some(json!({ "url": url }))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{err}");
}

#[test]
fn binding_beyond_loopback_requires_api_keys() {
    let local: SocketAddr = "127.0.0.1:3002".parse().unwrap();
    let public: SocketAddr = "0.0.0.0:3002".parse().unwrap();
    assert!(check_bind(local, &[]).is_ok());
    assert!(check_bind(public, &[]).is_err());
    assert!(check_bind(public, &["k".to_string()]).is_ok());
}
