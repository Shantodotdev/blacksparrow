//! Phase 6: agent-mode CLI commands (`scrape`, `map`, `crawl`, `find`, `extract`, `interact`)
//! parsed by clap and run end to end through the real binary.

use blacksparrow::cli::args::{Cli, Commands};
use clap::Parser;
use serde_json::Value;
use std::process::Output;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn site() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/robots.txt"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let pages = [
        (
            "/",
            "Home",
            "<p>Welcome to the garden shop, with seeds and tools.</p>\
             <a href=\"/seeds\">Seeds</a> <a href=\"/tools\">Tools</a>",
        ),
        (
            "/seeds",
            "Tomato seeds",
            "<p>Heirloom tomato seeds, 50 per pack.</p><p class=\"price\">Price: $4.99</p>",
        ),
        (
            "/tools",
            "Garden tools",
            "<p>A trowel and a fork, made from stainless steel.</p>",
        ),
    ];
    for (route, title, body) in pages {
        Mock::given(method("GET"))
            .and(path(route))
            .respond_with(ResponseTemplate::new(200).set_body_raw(
                format!(
                    "<html lang=\"en\"><head><title>{title}</title></head><body>\
                     <nav><a href=\"/\">Home</a></nav><main><h1>{title}</h1>{body}</main></body></html>"
                ),
                "text/html",
            ))
            .mount(&server)
            .await;
    }
    server
}

async fn run(args: Vec<String>) -> Output {
    tokio::task::spawn_blocking(move || {
        std::process::Command::new(env!("CARGO_BIN_EXE_blacksparrow"))
            .args(&args)
            .env("RUST_LOG", "warn")
            .output()
            .unwrap()
    })
    .await
    .unwrap()
}

fn common(server: &MockServer, db: &std::path::Path) -> Vec<String> {
    vec![
        "-a".into(),
        server.address().to_string(),
        "--db-path".into(),
        db.display().to_string(),
    ]
}

fn stdout(out: &Output) -> String {
    assert!(
        out.status.success(),
        "exit {:?}\nstdout: {}\nstderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn agent_commands_parse() {
    let cli = Cli::try_parse_from([
        "blacksparrow",
        "scrape",
        "https://example.com",
        "-f",
        "markdown,links",
        "--render",
        "never",
        "-a",
        "localhost:3000",
    ])
    .unwrap();
    match cli.command {
        Commands::Scrape(args) => {
            assert_eq!(args.url, "https://example.com");
            assert_eq!(args.page.format, "markdown,links");
            assert_eq!(args.web.allowed_hosts, vec!["localhost:3000"]);
        }
        other => panic!("parsed as {other:?}"),
    }

    let cli = Cli::try_parse_from([
        "blacksparrow",
        "crawl",
        "https://example.com",
        "--limit",
        "20",
        "--out",
        "./site",
        "--include-path",
        "/blog/*",
    ])
    .unwrap();
    assert!(
        matches!(cli.command, Commands::Crawl(ref a) if a.limit == 20 && a.include_paths == vec!["/blog/*"])
    );

    let cli = Cli::try_parse_from(["blacksparrow", "mcp", "--tools", "web"]).unwrap();
    assert!(matches!(cli.command, Commands::Mcp(ref a) if a.tools == "web"));
    let cli = Cli::try_parse_from(["blacksparrow", "mcp"]).unwrap();
    assert!(matches!(cli.command, Commands::Mcp(ref a) if a.tools == "all"));

    assert!(Cli::try_parse_from([
        "blacksparrow",
        "find",
        "https://example.com",
        "--query",
        "x"
    ])
    .is_ok());
    assert!(Cli::try_parse_from([
        "blacksparrow",
        "extract",
        "https://example.com",
        "--schema",
        "{}"
    ])
    .is_ok());
    assert!(Cli::try_parse_from([
        "blacksparrow",
        "interact",
        "https://example.com",
        "--steps",
        "[]"
    ])
    .is_ok());
}

#[tokio::test]
async fn scrape_prints_markdown_or_json() {
    let server = site().await;
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("cli.db");

    let mut args = vec!["scrape".to_string(), format!("{}/seeds", server.uri())];
    args.extend(common(&server, &db));
    let md = stdout(&run(args.clone()).await);
    assert!(md.contains("# Tomato seeds"), "{md}");
    assert!(md.contains("Heirloom tomato seeds"));
    assert!(!md.contains("Home"), "navigation is not main content");

    args.push("--json".into());
    let doc: Value = serde_json::from_str(&stdout(&run(args).await)).unwrap();
    assert_eq!(doc["status"], "ok");
    assert_eq!(doc["metadata"]["title"], "Tomato seeds");
}

#[tokio::test]
async fn map_lists_urls_one_per_line() {
    let server = site().await;
    let dir = tempfile::tempdir().unwrap();
    let mut args = vec![
        "map".to_string(),
        server.uri(),
        "--sitemap".into(),
        "skip".into(),
    ];
    args.extend(common(&server, &dir.path().join("cli.db")));
    let out = stdout(&run(args).await);
    let lines: Vec<&str> = out.lines().collect();
    assert!(
        lines.contains(&format!("{}/seeds", server.uri()).as_str()),
        "{out}"
    );
    assert!(lines.contains(&format!("{}/tools", server.uri()).as_str()));
}

#[tokio::test]
async fn crawl_writes_markdown_files_or_ndjson() {
    let server = site().await;
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("cli.db");
    let out_dir = dir.path().join("site");

    let mut args = vec![
        "crawl".to_string(),
        format!("{}/", server.uri()),
        "--out".into(),
        out_dir.display().to_string(),
    ];
    args.extend(common(&server, &db));
    let out = run(args).await;
    stdout(&out);
    let host_dir = out_dir.join(server.address().to_string().replace(':', "_"));
    let seeds = std::fs::read_to_string(host_dir.join("seeds.md")).unwrap();
    assert!(seeds.contains("Heirloom tomato seeds"));
    assert!(host_dir.join("index.md").exists());
    assert!(host_dir.join("tools.md").exists());

    let mut args = vec!["crawl".to_string(), format!("{}/", server.uri())];
    args.extend(common(&server, &db));
    let ndjson = stdout(&run(args).await);
    let docs: Vec<Value> = ndjson
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(docs.len(), 3, "{ndjson}");
    assert!(docs.iter().all(|d| d["status"] == "ok"));
}

#[tokio::test]
async fn find_and_extract_print_json() {
    let server = site().await;
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("cli.db");

    let mut args = vec![
        "find".to_string(),
        format!("{}/tools", server.uri()),
        "--query".into(),
        "what are the tools made of".into(),
    ];
    args.extend(common(&server, &db));
    let found: Value = serde_json::from_str(&stdout(&run(args).await)).unwrap();
    assert!(found["hits"][0]["text"]
        .as_str()
        .unwrap()
        .contains("stainless steel"));

    let schema = r#"{"type":"object","properties":{"title":{"type":"string"},"price":{"type":"number","x-kind":"price"}}}"#;
    let mut args = vec![
        "extract".to_string(),
        format!("{}/seeds", server.uri()),
        "--schema".into(),
        schema.into(),
    ];
    args.extend(common(&server, &db));
    let extracted: Value = serde_json::from_str(&stdout(&run(args).await)).unwrap();
    assert_eq!(extracted["data"]["title"], "Tomato seeds");
    assert_eq!(extracted["data"]["price"], 4.99);
}

#[tokio::test]
async fn refused_urls_exit_with_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let out = run(vec![
        "scrape".into(),
        "http://169.254.169.254/latest/meta-data/".into(),
        "--db-path".into(),
        dir.path().join("cli.db").display().to_string(),
    ])
    .await;
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("metadata"));
}
