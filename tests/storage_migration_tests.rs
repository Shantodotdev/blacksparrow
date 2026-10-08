//! Versioned SQLite migrations (`PRAGMA user_version`) and agent-mode document storage.

use blacksparrow::extract::{Block, BlockKind, PageDocument, PageStatus};
use blacksparrow::storage::documents::{latest_document, save_document, search_chunks, ChunkQuery};
use blacksparrow::storage::{sqlite, Database, LATEST_SCHEMA_VERSION, SCHEMA};
use rusqlite::Connection;

fn temp_db(name: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(name);
    (dir, path)
}

fn table_exists(conn: &Connection, name: &str) -> bool {
    conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE name = ?1",
        [name],
        |r| r.get::<_, i64>(0),
    )
    .unwrap_or(0)
        > 0
}

#[test]
fn bundled_sqlite_has_fts5() {
    let conn = Connection::open_in_memory().unwrap();
    let enabled: i64 = conn
        .query_row("SELECT sqlite_compileoption_used('ENABLE_FTS5')", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(enabled, 1);
}

#[test]
fn fresh_database_reaches_latest_version() {
    let (_dir, path) = temp_db("fresh.db");
    Database::open(&path).unwrap();
    let conn = Connection::open(&path).unwrap();
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, LATEST_SCHEMA_VERSION);
    for table in [
        "crawls",
        "pages",
        "documents",
        "chunks",
        "content_crawls",
        "extraction_rules",
    ] {
        assert!(table_exists(&conn, table), "missing table {table}");
    }
}

#[test]
fn legacy_database_upgrades_and_keeps_its_data() {
    let (_dir, path) = temp_db("legacy.db");
    {
        // A database created by an older release: unversioned, SEO tables only.
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        conn.execute(
            "INSERT INTO crawls (session_id, target_url, status, started_at) VALUES ('old', 'https://example.com/', 'completed', '2026-01-01')",
            [],
        )
        .unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 0);
    }

    Database::open(&path).unwrap();
    // Opening twice is a no-op.
    Database::open(&path).unwrap();

    let conn = Connection::open(&path).unwrap();
    let version: i64 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, LATEST_SCHEMA_VERSION);
    let kept: String = conn
        .query_row(
            "SELECT target_url FROM crawls WHERE session_id = 'old'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(kept, "https://example.com/");
    assert!(table_exists(&conn, "documents"));
}

fn sample_document(url: &str, body: &str) -> PageDocument {
    let mut doc = PageDocument::with_status(url, PageStatus::Ok);
    doc.metadata.title = Some("Pricing".into());
    doc.markdown = format!("# Pricing\n\n{body}\n");
    doc.content_hash = blacksparrow::parser::content::compute_content_hash(&doc.markdown);
    doc.blocks = vec![
        Block {
            kind: BlockKind::Heading { level: 1 },
            text: "Pricing".into(),
            heading_path: vec![],
            selector: "h1".into(),
            markdown: "# Pricing".into(),
        },
        Block {
            kind: BlockKind::Paragraph,
            text: body.into(),
            heading_path: vec!["Pricing".into()],
            selector: "p".into(),
            markdown: body.into(),
        },
    ];
    doc
}

#[test]
fn documents_round_trip_track_changes_and_feed_full_text_search() {
    let (_dir, path) = temp_db("docs.db");
    Database::open(&path).unwrap();
    let conn = sqlite::connect_configured(&path).unwrap();

    let first = sample_document(
        "https://example.com/pricing",
        "The Pro plan costs $49 per month.",
    );
    let stored = save_document(&conn, Some("job1"), &first).unwrap();
    assert_eq!(
        stored.changed, None,
        "first copy has nothing to compare against"
    );

    let same = save_document(&conn, Some("job2"), &first).unwrap();
    assert_eq!(same.changed, Some(false));

    let edited = sample_document(
        "https://example.com/pricing",
        "The Pro plan costs $59 per month.",
    );
    let edited = save_document(&conn, Some("job3"), &edited).unwrap();
    assert_eq!(edited.changed, Some(true));

    let (latest, _) = latest_document(&conn, "https://example.com/pricing")
        .unwrap()
        .expect("stored");
    assert!(latest.markdown.contains("$59"));
    assert_eq!(latest.blocks.len(), 2);

    let hits = search_chunks(
        &conn,
        &ChunkQuery {
            query: "pro plan cost".into(),
            crawl_id: Some("job3".into()),
            top_k: 3,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(hits.len(), 1, "only job3's chunks match the crawl filter");
    assert!(hits[0].text.contains("$59"));
    assert_eq!(hits[0].url, "https://example.com/pricing");
    assert_eq!(hits[0].heading_path, vec!["Pricing".to_string()]);
}
