//! # Extract Output Schema Tests
//!
//! The JSON Schema printed by `blacksparrow schema extract` and committed as
//! `schema/extract.schema.json` is the contract the Python and JavaScript SDKs are built on.
//! These tests keep the committed copy in step with the Rust types and prove that real
//! `extract` output, served from the fixture sites, validates against it.

use blacksparrow::extract::render::RenderOptions;
use blacksparrow::extract::{extract_site, output_schema, ExtractOptions, RenderMode};
use jsonschema::Validator;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sites")
}

fn committed_schema() -> Value {
    let file = Path::new(env!("CARGO_MANIFEST_DIR")).join("schema/extract.schema.json");
    serde_json::from_slice(&std::fs::read(file).expect("schema/extract.schema.json"))
        .expect("committed schema is JSON")
}

fn validator() -> Validator {
    jsonschema::validator_for(&committed_schema()).expect("committed schema compiles")
}

fn assert_valid(validator: &Validator, document: &Value) {
    let errors: Vec<String> = validator
        .iter_errors(document)
        .map(|e| format!("{} at {}", e, e.instance_path()))
        .collect();
    assert!(errors.is_empty(), "{errors:#?}\n{document:#}");
}

/// Mounts every file under `tests/fixtures/sites/<site>` at its relative path; `index.html`
/// is also served at `/`.
async fn serve_site(server: &MockServer, site: &str) {
    let root = fixtures().join(site);
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let file = entry.unwrap().path();
            if file.is_dir() {
                stack.push(file);
                continue;
            }
            let relative = file
                .strip_prefix(&root)
                .unwrap()
                .to_str()
                .unwrap()
                .replace('\\', "/");
            let content_type = match relative.rsplit('.').next().unwrap_or("") {
                "html" => "text/html; charset=utf-8",
                "css" => "text/css",
                "js" => "application/javascript",
                _ => "application/octet-stream",
            };
            let template = ResponseTemplate::new(200)
                .set_body_bytes(std::fs::read(&file).unwrap())
                .insert_header("content-type", content_type);
            let mut mounts = vec![format!("/{relative}")];
            if relative == "index.html" {
                mounts.push("/".to_string());
            }
            for url_path in mounts {
                Mock::given(method("GET"))
                    .and(path(url_path))
                    .respond_with(template.clone())
                    .mount(server)
                    .await;
            }
        }
    }
}

fn options(render: RenderMode) -> ExtractOptions {
    let mut options = ExtractOptions::new();
    options.allow_private_network = true;
    options.render = render;
    options.timeout = Duration::from_secs(60);
    options.request_timeout = Duration::from_secs(10);
    options
}

async fn extract_json(server: &MockServer, options: ExtractOptions) -> Value {
    let result = extract_site(&server.uri(), options).await.unwrap();
    serde_json::to_value(&result).unwrap()
}

#[test]
fn committed_schema_matches_the_rust_types() {
    let generated = serde_json::to_value(output_schema()).unwrap();
    assert_eq!(
        generated,
        committed_schema(),
        "schema/extract.schema.json is stale; regenerate it with \
         `cargo run -- schema extract > schema/extract.schema.json`"
    );
}

#[test]
fn schema_is_draft_2020_12_for_schema_version_one() {
    let schema = committed_schema();
    assert_eq!(
        schema["$schema"],
        "https://json-schema.org/draft/2020-12/schema"
    );
    assert_eq!(schema["title"], "SiteExtract");
    assert_eq!(schema["type"], "object");
    for field in ["schema_version", "outcome", "pages", "design", "stats"] {
        assert!(
            schema["required"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r == field),
            "{field} should be required"
        );
    }
}

#[test]
fn schema_rejects_documents_that_break_the_contract() {
    let validator = validator();
    let server_less_failure = serde_json::json!({
        "schema_version": 1,
        "generator": "blacksparrow 0.1.0",
        "start_url": "https://example.com",
        "final_url": null,
        "outcome": "unreachable",
        "pages": [],
        "page_errors": [],
        "design": null,
        "contacts": {"phones": [], "emails": [], "whatsapp": [], "facebook": [], "instagram": []},
        "structured": {
            "names": [], "telephones": [], "emails": [], "addresses": [],
            "same_as": [], "logos": [], "descriptions": []
        },
        "stats": {
            "elapsed_ms": 0, "http_fetches": 0, "http_bytes": 0, "rendered_pages": 0,
            "render_ms": 0, "blocked_browser_requests": 0
        }
    });
    assert_valid(&validator, &server_less_failure);

    let mut unknown_outcome = server_less_failure.clone();
    unknown_outcome["outcome"] = "exploded".into();
    assert!(!validator.is_valid(&unknown_outcome));

    let mut missing_design = server_less_failure.clone();
    missing_design.as_object_mut().unwrap().remove("design");
    assert!(!validator.is_valid(&missing_design));

    let mut wrong_type = server_less_failure;
    wrong_type["stats"]["http_fetches"] = "many".into();
    assert!(!validator.is_valid(&wrong_type));
}

#[tokio::test]
async fn bakery_extract_validates() {
    let server = MockServer::start().await;
    serve_site(&server, "bakery").await;
    let document = extract_json(&server, options(RenderMode::Never)).await;
    assert_eq!(document["outcome"], "ok");
    assert!(!document["pages"].as_array().unwrap().is_empty());
    assert!(!document["design"]["colors"].as_array().unwrap().is_empty());
    assert_valid(&validator(), &document);
}

#[tokio::test]
async fn unrendered_spa_extract_validates() {
    let server = MockServer::start().await;
    serve_site(&server, "spa").await;
    let document = extract_json(&server, options(RenderMode::Never)).await;
    assert_valid(&validator(), &document);
}

#[tokio::test]
async fn rendered_spa_extract_validates_when_chromium_is_available() {
    let Ok(chrome) = std::env::var("BLACKSPARROW_TEST_CHROME") else {
        eprintln!("skipped: BLACKSPARROW_TEST_CHROME is not set");
        return;
    };
    let server = MockServer::start().await;
    serve_site(&server, "spa").await;
    let mut opts = options(RenderMode::Auto);
    opts.render_options = RenderOptions {
        chrome_path: Some(PathBuf::from(chrome)),
        no_sandbox: true,
        page_timeout: Duration::from_secs(25),
        ..RenderOptions::default()
    };
    let document = extract_json(&server, opts).await;
    assert_eq!(document["pages"][0]["rendered"], true);
    assert_eq!(document["pages"][0]["render_reason"], "js_shell");
    assert_valid(&validator(), &document);
}

#[tokio::test]
async fn failed_extractions_validate() {
    let validator = validator();

    // A challenge page: bot protected, with a WAF name on the page.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_bytes(std::fs::read(fixtures().join("hostile/challenge.html")).unwrap())
                .insert_header("content-type", "text/html; charset=utf-8")
                .insert_header("cf-mitigated", "challenge")
                .insert_header("server", "cloudflare"),
        )
        .mount(&server)
        .await;
    let challenge = extract_json(&server, options(RenderMode::Never)).await;
    assert_eq!(challenge["outcome"], "bot_protected");
    assert_valid(&validator, &challenge);

    // A homepage that is not HTML.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(vec![0x89, b'P', b'N', b'G'])
                .insert_header("content-type", "image/png"),
        )
        .mount(&server)
        .await;
    let not_html = extract_json(&server, options(RenderMode::Never)).await;
    assert_eq!(not_html["outcome"], "not_html");
    assert_valid(&validator, &not_html);

    // A refused address: nothing was read, so there is no design and no final URL.
    let server = MockServer::start().await;
    serve_site(&server, "bakery").await;
    let mut refused = options(RenderMode::Never);
    refused.allow_private_network = false;
    let blocked = extract_json(&server, refused).await;
    assert_eq!(blocked["outcome"], "blocked_address");
    assert!(blocked["design"].is_null() && blocked["final_url"].is_null());
    assert_valid(&validator, &blocked);

    // Malformed input URL.
    let result = extract_site("not a url", options(RenderMode::Never))
        .await
        .unwrap();
    let malformed = serde_json::to_value(&result).unwrap();
    assert_eq!(malformed["outcome"], "unsupported_url");
    assert_valid(&validator, &malformed);
}

#[tokio::test]
async fn hostile_pages_validate() {
    let server = MockServer::start().await;
    serve_site(&server, "hostile").await;
    let document = extract_json(&server, options(RenderMode::Never)).await;
    assert_valid(&validator(), &document);
}

#[tokio::test]
async fn cli_output_validates_pretty_and_compact() {
    let server = MockServer::start().await;
    serve_site(&server, "bakery").await;
    let url = server.uri();
    let outputs = tokio::task::spawn_blocking(move || {
        [Some("--pretty"), None].map(|flag| {
            let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_blacksparrow"));
            command.args([
                "extract",
                &url,
                "--allow-private-network",
                "--render",
                "never",
            ]);
            command.args(flag);
            command.output().unwrap()
        })
    })
    .await
    .unwrap();
    let validator = validator();
    for output in outputs {
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let document: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_valid(&validator, &document);
    }
}

#[test]
fn schema_command_prints_the_committed_schema() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_blacksparrow"))
        .args(["schema", "extract"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let printed: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(printed, committed_schema());
}
