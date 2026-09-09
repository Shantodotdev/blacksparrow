//! # Storage Subsystem & Real-Time Persistence Integration Tests
//!
//! Validates SQLite database initialization, WAL mode enforcement, schema migrations,
//! real-time asynchronous batch writing, transactional flushes, and historical session queries.

use blacksparrow::core::models::{
    DiscoveredLink, HreflangTag, ImageResource, IssueCategory, IssueFinding, PageReport,
    RobotsFlags, RuleId, SchemaRecord, Severity,
};
use blacksparrow::storage::{CrawlSessionInit, Database};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

static TEST_COUNTER: AtomicUsize = AtomicUsize::new(1);

fn unique_test_db_path() -> PathBuf {
    let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let mut dir = std::env::temp_dir();
    dir.push(format!("seolens_test_{}_{}.db", std::process::id(), id));
    dir
}

fn create_sample_page(crawl_id: &str, url: &str, status_code: u16) -> PageReport {
    PageReport {
        crawl_id: crawl_id.into(),
        url: url.to_string(),
        url_hash: 123456789,
        final_url: Some(url.to_string()),
        status_code,
        content_type: "text/html; charset=utf-8".into(),
        size_bytes: 5120,
        ttfb_ms: 120,
        crawl_depth: 1,
        title: Some("Sample Title for Test".to_string()),
        title_length: 21,
        meta_description: Some("Meta description for the sample test page.".to_string()),
        meta_desc_length: 42,
        canonical_url: Some(url.to_string()),
        html_lang: Some("en".into()),
        robots_flags: RobotsFlags::NONE,
        is_sitemap_url: true,
        is_internal: true,
        h1_primary: Some("Main Heading".to_string()),
        h1_count: 1,
        h2_headings: vec!["Section 1".to_string(), "Section 2".to_string()],
        h3_headings: vec!["Subsection A".to_string()],
        word_count: 350,
        content_hash: 987654321,
        simhash: 1122334455,
        is_https: url.starts_with("https://"),
        has_hsts: true,
        has_csp: true,
        has_x_frame: true,
        has_x_content_type: true,
        links: vec![DiscoveredLink {
            source_url: url.to_string(),
            target_url: format!("{}/about", url),
            target_url_hash: 999888777,
            anchor_text: "About Us".to_string(),
            is_internal: true,
            is_nofollow: false,
            is_image_link: false,
            is_target_blank: false,
            has_opener_or_referrer: true,
            status_code: Some(200),
        }],
        images: vec![ImageResource {
            src_url: format!("{}/logo.png", url),
            alt_text: Some("Company Logo".to_string()),
            width: Some(200),
            height: Some(60),
            size_bytes: Some(8192),
            has_dimensions: true,
            is_broken: false,
        }],
        schemas: vec![SchemaRecord {
            schema_type: "Organization".into(),
            raw_json: r#"{"@type":"Organization","name":"SEO Lens"}"#.to_string(),
            is_valid_json: true,
            is_google_eligible: true,
            missing_required_fields: vec![],
        }],
        hreflangs: vec![HreflangTag {
            lang_code: "en".into(),
            target_url: url.to_string(),
            is_reciprocal: true,
        }],
        issues: vec![IssueFinding {
            code: RuleId::WarnTitleTooShort,
            category: IssueCategory::TitleMetadata,
            severity: Severity::Warning,
            title: "Title Too Short".into(),
            message: "The page title is shorter than recommended.".to_string(),
            target_url: url.to_string(),
            source_page_url: None,
        }],
        ..Default::default()
    }
}

#[tokio::test]
async fn test_sqlite_initialization_and_schema() {
    let db_path = unique_test_db_path();
    let db = Database::open(&db_path).expect("Database initialization must succeed");

    // Verify session creation
    let init = CrawlSessionInit {
        session_id: "test-session-init".to_string(),
        target_url: "https://example.com/".to_string(),
        max_pages: 500,
        max_depth: 5,
        respect_robots: true,
        render_js: false,
    };
    db.init_crawl_session(&init)
        .expect("Creating session must succeed");

    let sessions = db.list_crawls().expect("Listing crawls must succeed");
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].session_id, "test-session-init");
    assert_eq!(sessions[0].target_url, "https://example.com/");

    // Cleanup
    let _ = std::fs::remove_file(&db_path);
    let mut shm = db_path.clone();
    shm.set_extension("db-shm");
    let _ = std::fs::remove_file(shm);
    let mut wal = db_path.clone();
    wal.set_extension("db-wal");
    let _ = std::fs::remove_file(wal);
}

#[tokio::test]
async fn test_realtime_batch_writer_and_flush() {
    let db_path = unique_test_db_path();
    let db = Arc::new(Database::open(&db_path).expect("DB open"));

    let session_id = "realtime-session-1";
    db.init_crawl_session(&CrawlSessionInit {
        session_id: session_id.to_string(),
        target_url: "https://mysite.com/".to_string(),
        max_pages: 500,
        max_depth: 3,
        respect_robots: true,
        render_js: false,
    })
    .expect("Init session");

    // Spawn writer actor with batch size 50 and 500ms flush timer
    let (writer_handle, writer_task) = db
        .spawn_writer(session_id, 50, Duration::from_millis(500))
        .expect("Spawn writer");

    // Stream 120 pages in real-time
    for i in 1..=120 {
        let page = create_sample_page(
            session_id,
            &format!("https://mysite.com/page-{}", i),
            if i % 10 == 0 { 404 } else { 200 },
        );
        writer_handle
            .save_page(page)
            .await
            .expect("Saving page via channel must succeed");
    }

    // Explicit flush
    writer_handle.flush().await.expect("Flush must succeed");

    // Check count in database while writer is still running
    let pages = db
        .get_crawl_pages(session_id, 200, 0)
        .expect("Querying pages");
    assert_eq!(pages.len(), 120);

    // Verify child tables persisted
    assert_eq!(pages[0].links.len(), 1);
    assert_eq!(pages[0].images.len(), 1);
    assert_eq!(pages[0].schemas.len(), 1);
    assert_eq!(pages[0].hreflangs.len(), 1);
    assert_eq!(pages[0].issues.len(), 1);

    // Shutdown writer actor cleanly
    writer_handle.shutdown().await.expect("Shutdown writer");
    writer_task
        .await
        .expect("Writer task join")
        .expect("Writer task result");

    // Cleanup
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn test_interrupted_session_preservation() {
    let db_path = unique_test_db_path();
    let db = Arc::new(Database::open(&db_path).expect("DB open"));

    let session_id = "interrupted-session-test";
    db.init_crawl_session(&CrawlSessionInit {
        session_id: session_id.to_string(),
        target_url: "https://large-enterprise.com/".to_string(),
        max_pages: 10000,
        max_depth: 10,
        respect_robots: true,
        render_js: false,
    })
    .expect("Init session");

    let (writer_handle, writer_task) = db
        .spawn_writer(session_id, 25, Duration::from_millis(200))
        .expect("Spawn writer");

    // Crawl 35 pages before user simulates Ctrl+C
    for i in 1..=35 {
        let page = create_sample_page(
            session_id,
            &format!("https://large-enterprise.com/product-{}", i),
            200,
        );
        writer_handle.save_page(page).await.expect("Save page");
    }

    // User interrupts (Ctrl+C). Crawler flushes writer and updates status to "interrupted"
    writer_handle.flush().await.expect("Flush");
    db.update_crawl_status(
        session_id,
        "interrupted",
        Some("2026-09-06T12:00:00Z"),
        35,
        0,
        0,
        35,
        Some(88),
    )
    .expect("Update interrupted status");

    writer_handle.shutdown().await.expect("Shutdown");
    writer_task.await.expect("Join").expect("Result");

    // Verify the session is persisted and inspectable
    let crawl = db
        .get_crawl(session_id)
        .expect("Get crawl")
        .expect("Crawl exists");
    assert_eq!(crawl.session_id, session_id);
    assert_eq!(crawl.total_pages_crawled, 35);
    assert_eq!(crawl.health_score, 88);

    let issues = db
        .get_crawl_issues(session_id, None, None)
        .expect("Get crawl issues");
    assert_eq!(issues.len(), 35);

    // Test severity filter
    let warnings = db
        .get_crawl_issues(session_id, Some(Severity::Warning), None)
        .expect("Get warnings");
    assert_eq!(warnings.len(), 35);

    let criticals = db
        .get_crawl_issues(session_id, Some(Severity::Critical), None)
        .expect("Get criticals");
    assert_eq!(criticals.len(), 0);

    // Cleanup
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn test_crawl_with_live_sqlite_persistence_and_cancellation() {
    use blacksparrow::core::config::CrawlConfig;
    use blacksparrow::crawler::run_crawl_with_options;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let mock_server = MockServer::start().await;

    // Build a mock website with multiple linked pages
    for i in 1..=10 {
        let current_path = if i == 1 {
            "/".to_string()
        } else {
            format!("/page-{}", i)
        };
        let next_path = format!("/page-{}", i + 1);
        let html = format!(
            r#"<!DOCTYPE html><html><head><title>Page {}</title></head><body><h1>Heading {}</h1><a href="{}">Next</a></body></html>"#,
            i, i, next_path
        );

        Mock::given(method("GET"))
            .and(path(&current_path))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(html)
                    .insert_header("content-type", "text/html; charset=utf-8"),
            )
            .mount(&mock_server)
            .await;
    }

    let db_path = unique_test_db_path();
    let db = Arc::new(Database::open(&db_path).expect("DB open"));
    let session_id = "live-crawling-interrupted";

    db.init_crawl_session(&CrawlSessionInit {
        session_id: session_id.to_string(),
        target_url: mock_server.uri(),
        max_pages: 50,
        max_depth: 10,
        respect_robots: false,
        render_js: false,
    })
    .expect("Init session");

    let (writer_handle, writer_task) = db
        .spawn_writer(session_id, 2, Duration::from_millis(50))
        .expect("Spawn writer");

    let mut config = CrawlConfig::new(&mock_server.uri()).expect("Config");
    config.max_pages = 50;
    config.respect_robots = false;
    config.session_id = Some(session_id.to_string());

    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
    let cancel_tx = Arc::new(std::sync::Mutex::new(Some(cancel_tx)));

    // Cancel crawl as soon as 2 pages have been crawled
    let progress_cb = Arc::new(move |update: blacksparrow::crawler::ProgressUpdate| {
        if update.crawled_pages >= 2 {
            if let Ok(mut lock) = cancel_tx.lock() {
                if let Some(tx) = lock.take() {
                    let _ = tx.send(());
                }
            }
        }
    });

    let crawl_result = run_crawl_with_options(
        &config,
        Some(progress_cb),
        Some(writer_handle.clone()),
        Some(cancel_rx),
    )
    .await
    .expect("Crawl run");

    writer_handle.shutdown().await.expect("Writer shutdown");
    writer_task.await.expect("Join").expect("Result");

    // Assert that pages crawled >= 2 and all are saved in SQLite
    assert!(crawl_result.pages.len() >= 2);
    let saved_pages = db
        .get_crawl_pages(session_id, 100, 0)
        .expect("Query saved pages");
    assert_eq!(saved_pages.len(), crawl_result.pages.len());

    let session = db
        .get_crawl(session_id)
        .expect("Get crawl")
        .expect("Session exists");
    assert_eq!(session.total_pages_crawled, crawl_result.pages.len() as u32);
    assert!(session.health_score > 0);

    // Cleanup
    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn test_cli_commands_list_and_report() {
    use blacksparrow::cli::args::{Cli, Commands, ListArgs, ReportArgs};
    use blacksparrow::cli::commands::execute;

    let db_path = unique_test_db_path();
    let db = Database::open(&db_path).expect("DB open");
    let session_id = "test-cli-report-session";

    db.init_crawl_session(&CrawlSessionInit {
        session_id: session_id.to_string(),
        target_url: "https://my-stored-site.com/".to_string(),
        max_pages: 10,
        max_depth: 2,
        respect_robots: true,
        render_js: false,
    })
    .expect("Init session");

    // Add a page and finalize status
    let (writer_handle, writer_task) = db
        .spawn_writer(session_id, 1, Duration::from_millis(50))
        .expect("Spawn writer");

    let page = create_sample_page(session_id, "https://my-stored-site.com/", 200);
    writer_handle.save_page(page).await.expect("Save page");
    writer_handle.flush().await.expect("Flush");
    db.update_crawl_status(
        session_id,
        "completed",
        Some("2026-09-06T14:00:00Z"),
        1,
        0,
        0,
        1,
        Some(95),
    )
    .expect("Update status");

    writer_handle.shutdown().await.expect("Shutdown");
    writer_task.await.expect("Join").expect("Result");

    // 1. Test List command
    let list_cli = Cli {
        command: Commands::List(ListArgs {
            db_path: Some(db_path.clone()),
            ..Default::default()
        }),
    };
    execute(list_cli).await.expect("List command execution");

    // 2. Test Report command re-exporting to JSON
    let temp_reports = std::env::temp_dir().join(format!("seolens_rep_{}", std::process::id()));
    let report_cli = Cli {
        command: Commands::Report(ReportArgs {
            session: Some(session_id.to_string()),
            session_pos: None,
            format: Some("json".to_string()),
            output_dir: Some(temp_reports.clone()),
            db_path: Some(db_path.clone()),
            ..Default::default()
        }),
    };
    execute(report_cli).await.expect("Report command execution");

    let json_file = temp_reports.join("my-stored-site_com_audit.json");
    assert!(json_file.exists(), "Re-exported JSON report should exist");

    // Cleanup
    let _ = std::fs::remove_dir_all(&temp_reports);
    let _ = std::fs::remove_file(&db_path);
}

#[test]
fn test_database_path_resolution() {
    use blacksparrow::storage::{default_db_path, local_db_path, resolve_db_path};

    // 1. local_db_path returns .blacksparrow/blacksparrow.db (or legacy .seolens/seolens.db)
    let local = local_db_path();
    assert!(
        local == PathBuf::from(".blacksparrow").join("blacksparrow.db")
            || local == PathBuf::from(".seolens").join("seolens.db")
    );

    // 2. Explicit path takes highest precedence
    let explicit = PathBuf::from("/custom/db/path.sqlite");
    assert_eq!(resolve_db_path(Some(explicit.clone()), false), explicit);
    assert_eq!(resolve_db_path(Some(explicit.clone()), true), explicit);

    // 3. Local flag forces local_db_path when no explicit path given
    assert_eq!(resolve_db_path(None, true), local);

    // 4. BLACKSPARROW_DB_PATH environment variable override
    let orig_env = std::env::var("BLACKSPARROW_DB_PATH").ok();
    let temp_env_path = std::env::temp_dir().join("test_env_blacksparrow.db");
    std::env::set_var("BLACKSPARROW_DB_PATH", temp_env_path.to_str().unwrap());

    assert_eq!(default_db_path(), temp_env_path);
    assert_eq!(resolve_db_path(None, false), temp_env_path);

    // Restore or unset env var
    match orig_env {
        Some(val) => std::env::set_var("BLACKSPARROW_DB_PATH", val),
        None => std::env::remove_var("BLACKSPARROW_DB_PATH"),
    }

    // 5. Without env var, standard resolution returns OS data dir (if it exists)
    if std::env::var("BLACKSPARROW_DB_PATH").is_err() && !local.exists() {
        if let Some(expected_os_dir) = dirs::data_dir() {
            let expected_new = expected_os_dir.join("blacksparrow").join("blacksparrow.db");
            let expected_legacy = expected_os_dir.join("seolens").join("seolens.db");
            let actual = default_db_path();
            assert!(actual == expected_new || actual == expected_legacy);
        }
    }
}
