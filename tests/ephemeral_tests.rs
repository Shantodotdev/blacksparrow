//! # Ephemeral Audit & Database Preservation Integration Tests
//!
//! Verifies that ephemeral audits run purely in-memory and never delete, mutate,
//! or corrupt pre-existing SQLite database files on disk.

use blacksparrow::cli::args::{AuditArgs, Cli, Commands};
use blacksparrow::cli::execute;
use blacksparrow::storage::{CrawlSessionInit, Database};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn get_temp_db_path(name: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let mut p = std::env::temp_dir();
    p.push(format!(
        "blacksparrow_test_{}_{}_{}.db",
        name,
        std::process::id(),
        nanos
    ));
    p
}

#[tokio::test]
async fn test_ephemeral_crawl_preserves_existing_database() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            r#"<!DOCTYPE html><html><head><title>Ephemeral Test</title></head><body><h1>Hello</h1></body></html>"#,
        ))
        .mount(&server)
        .await;

    let db_path = get_temp_db_path("preserve_ephemeral");

    // 1. Create a pre-existing database with real data
    {
        let db = Database::open(&db_path).expect("Failed to create pre-existing database");
        db.init_crawl_session(&CrawlSessionInit {
            session_id: "crawl_preserve_test_123".to_string(),
            target_url: "https://example.com".to_string(),
            max_pages: 10,
            max_depth: 2,
            respect_robots: true,
            render_js: false,
        })
        .expect("Failed to initialize session");
    }

    assert!(
        db_path.exists(),
        "Pre-existing database must exist before crawl"
    );
    let pre_size = fs::metadata(&db_path).expect("Read metadata").len();
    assert!(pre_size > 0, "Pre-existing database must not be empty");

    // 2. Run an ephemeral audit pointing db_path to the pre-existing database
    let cli = Cli {
        command: Commands::Audit(AuditArgs {
            url: server.uri(),
            ephemeral: true,
            db_path: Some(db_path.clone()),
            quiet: true,
            format: "none".to_string(),
            max_pages: 5,
            concurrency: 2,
            ..Default::default()
        }),
    };

    let result = execute(cli).await;
    assert!(
        result.is_ok(),
        "Ephemeral crawl should succeed: {:?}",
        result.err()
    );

    // 3. Verify the pre-existing database was NOT deleted
    assert!(
        db_path.exists(),
        "CRITICAL BUG: Pre-existing database at {:?} was deleted by ephemeral audit!",
        db_path
    );

    // 4. Verify the database contents and sessions are still intact
    let db = Database::open(&db_path).expect("Failed to re-open pre-existing database");
    let crawl = db
        .get_crawl("crawl_preserve_test_123")
        .expect("Query crawl session");
    assert!(
        crawl.is_some(),
        "Existing crawl session must still exist in the preserved database"
    );

    // Clean up test file
    let _ = fs::remove_file(&db_path);
}
