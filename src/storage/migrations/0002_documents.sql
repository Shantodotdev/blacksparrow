-- Agent-mode documents: one row per scraped or crawled page.
CREATE TABLE IF NOT EXISTS documents (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    crawl_id TEXT,
    url TEXT NOT NULL,
    url_hash INTEGER NOT NULL,
    final_url TEXT NOT NULL,
    host TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL,
    status_code INTEGER NOT NULL DEFAULT 0,
    title TEXT,
    content_hash INTEGER NOT NULL DEFAULT 0,
    tokens INTEGER NOT NULL DEFAULT 0,
    changed INTEGER,
    fetched_at INTEGER NOT NULL,
    document_json TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_documents_url ON documents(url_hash, fetched_at);
CREATE INDEX IF NOT EXISTS idx_documents_crawl ON documents(crawl_id);

-- Heading-based passages of every stored document, ranked with bm25().
CREATE VIRTUAL TABLE IF NOT EXISTS chunks USING fts5(
    text,
    heading_path,
    url UNINDEXED,
    host UNINDEXED,
    crawl_id UNINDEXED,
    document_id UNINDEXED,
    selector UNINDEXED,
    tokenize = 'porter unicode61'
);

-- Background content crawls (separate from SEO audit sessions).
CREATE TABLE IF NOT EXISTS content_crawls (
    id TEXT PRIMARY KEY,
    target_url TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('queued', 'crawling', 'completed', 'cancelled', 'failed')),
    options_json TEXT NOT NULL DEFAULT '{}',
    pages_done INTEGER NOT NULL DEFAULT 0,
    pages_failed INTEGER NOT NULL DEFAULT 0,
    pages_skipped INTEGER NOT NULL DEFAULT 0,
    error TEXT,
    created_at INTEGER NOT NULL,
    finished_at INTEGER
);
