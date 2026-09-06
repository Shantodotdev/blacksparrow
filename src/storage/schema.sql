PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;
PRAGMA busy_timeout = 5000;
PRAGMA cache_size = -64000;

CREATE TABLE IF NOT EXISTS crawls (
    session_id TEXT PRIMARY KEY,
    target_url TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('queued', 'crawling', 'completed', 'interrupted', 'failed')),
    total_max_pages INTEGER NOT NULL DEFAULT 500,
    max_depth INTEGER NOT NULL DEFAULT 5,
    respect_robots BOOLEAN NOT NULL DEFAULT 1,
    render_js BOOLEAN NOT NULL DEFAULT 0,
    started_at TEXT NOT NULL,
    finished_at TEXT,
    total_pages INTEGER NOT NULL DEFAULT 0,
    error_count INTEGER NOT NULL DEFAULT 0,
    alert_count INTEGER NOT NULL DEFAULT 0,
    warning_count INTEGER NOT NULL DEFAULT 0,
    health_score INTEGER DEFAULT NULL
);

CREATE TABLE IF NOT EXISTS pages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    crawl_id TEXT NOT NULL REFERENCES crawls(session_id) ON DELETE CASCADE,
    url TEXT NOT NULL,
    url_hash INTEGER NOT NULL,
    final_url TEXT,
    status_code INTEGER NOT NULL,
    content_type TEXT NOT NULL,
    size_bytes INTEGER NOT NULL DEFAULT 0,
    ttfb_ms INTEGER NOT NULL DEFAULT 0,
    crawl_depth INTEGER NOT NULL DEFAULT 0,
    title TEXT,
    title_length INTEGER NOT NULL DEFAULT 0,
    meta_description TEXT,
    meta_desc_length INTEGER NOT NULL DEFAULT 0,
    canonical_url TEXT,
    html_lang TEXT,
    charset TEXT,
    viewport TEXT,
    robots_flags INTEGER NOT NULL DEFAULT 0,
    is_sitemap_url BOOLEAN NOT NULL DEFAULT 0,
    is_internal BOOLEAN NOT NULL DEFAULT 1,
    h1_primary TEXT,
    h1_count INTEGER NOT NULL DEFAULT 0,
    h2_headings TEXT NOT NULL DEFAULT '[]',
    h3_headings TEXT NOT NULL DEFAULT '[]',
    word_count INTEGER NOT NULL DEFAULT 0,
    content_hash INTEGER NOT NULL DEFAULT 0,
    simhash INTEGER NOT NULL DEFAULT 0,
    is_soft_404 BOOLEAN NOT NULL DEFAULT 0,
    has_lorem_ipsum BOOLEAN NOT NULL DEFAULT 0,
    is_https BOOLEAN NOT NULL DEFAULT 1,
    has_hsts BOOLEAN NOT NULL DEFAULT 0,
    has_csp BOOLEAN NOT NULL DEFAULT 0,
    has_x_frame BOOLEAN NOT NULL DEFAULT 0,
    has_x_content_type BOOLEAN NOT NULL DEFAULT 0,
    mixed_content_count INTEGER NOT NULL DEFAULT 0,
    page_intent TEXT NOT NULL DEFAULT '{}',
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS links (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    crawl_id TEXT NOT NULL REFERENCES crawls(session_id) ON DELETE CASCADE,
    source_url TEXT NOT NULL,
    target_url TEXT NOT NULL,
    target_url_hash INTEGER NOT NULL,
    anchor_text TEXT NOT NULL DEFAULT '',
    is_internal BOOLEAN NOT NULL DEFAULT 1,
    is_nofollow BOOLEAN NOT NULL DEFAULT 0,
    is_image_link BOOLEAN NOT NULL DEFAULT 0,
    is_target_blank BOOLEAN NOT NULL DEFAULT 0,
    has_opener_or_referrer BOOLEAN NOT NULL DEFAULT 0,
    status_code INTEGER DEFAULT NULL
);

CREATE TABLE IF NOT EXISTS issues (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    crawl_id TEXT NOT NULL REFERENCES crawls(session_id) ON DELETE CASCADE,
    target_url TEXT NOT NULL,
    code TEXT NOT NULL,
    category TEXT NOT NULL,
    severity INTEGER NOT NULL CHECK(severity IN (1, 2, 3, 4)),
    title TEXT NOT NULL,
    message TEXT NOT NULL,
    source_page_url TEXT
);

CREATE TABLE IF NOT EXISTS schemas (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    crawl_id TEXT NOT NULL REFERENCES crawls(session_id) ON DELETE CASCADE,
    page_url TEXT NOT NULL,
    schema_type TEXT NOT NULL,
    raw_json TEXT NOT NULL,
    is_valid_json BOOLEAN NOT NULL DEFAULT 1,
    is_google_eligible BOOLEAN NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS images (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    crawl_id TEXT NOT NULL REFERENCES crawls(session_id) ON DELETE CASCADE,
    page_url TEXT NOT NULL,
    src_url TEXT NOT NULL,
    alt_text TEXT,
    width INTEGER,
    height INTEGER,
    size_bytes INTEGER,
    has_dimensions BOOLEAN NOT NULL DEFAULT 0,
    is_broken BOOLEAN NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS hreflangs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    crawl_id TEXT NOT NULL REFERENCES crawls(session_id) ON DELETE CASCADE,
    page_url TEXT NOT NULL,
    lang_code TEXT NOT NULL,
    target_url TEXT NOT NULL,
    is_reciprocal BOOLEAN NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS idx_pages_crawl_hash ON pages(crawl_id, url_hash);
CREATE INDEX IF NOT EXISTS idx_pages_status ON pages(crawl_id, status_code);
CREATE INDEX IF NOT EXISTS idx_links_target ON links(crawl_id, target_url_hash);
CREATE INDEX IF NOT EXISTS idx_issues_crawl_severity ON issues(crawl_id, severity);
CREATE INDEX IF NOT EXISTS idx_issues_code ON issues(crawl_id, code);
CREATE INDEX IF NOT EXISTS idx_schemas_crawl ON schemas(crawl_id);
CREATE INDEX IF NOT EXISTS idx_images_crawl ON images(crawl_id);
CREATE INDEX IF NOT EXISTS idx_hreflangs_crawl ON hreflangs(crawl_id);
