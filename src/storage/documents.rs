//! Agent-mode persistence: page documents, FTS5 passages, content crawl jobs and learned
//! extraction rules.

use crate::core::url::url_hash;
use crate::error::{SeoError, SeoResult};
use crate::extract::chunk::{chunk_blocks, DEFAULT_CHUNK_TOKENS};
use crate::extract::types::{unix_now, PageDocument};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

/// Result of storing a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoredDocument {
    /// Row id in `documents`.
    pub id: i64,
    /// Whether the content changed since the previous copy of the same URL (`None` = first copy).
    pub changed: Option<bool>,
}

/// Stores a document and its passages. Sets `changed` by comparing content hashes with the
/// previous copy of the same URL.
pub fn save_document(
    conn: &Connection,
    crawl_id: Option<&str>,
    doc: &PageDocument,
) -> SeoResult<StoredDocument> {
    let hash = url_hash(&doc.url) as i64;
    let previous: Option<i64> = conn
        .query_row(
            "SELECT content_hash FROM documents WHERE url_hash = ?1 AND url = ?2 AND status = 'ok'
             ORDER BY fetched_at DESC, id DESC LIMIT 1",
            params![hash, doc.url],
            |r| r.get(0),
        )
        .optional()?;
    let changed = if doc.status == crate::extract::PageStatus::Ok {
        previous.map(|p| p != doc.content_hash as i64)
    } else {
        None
    };

    let mut stored = doc.clone();
    stored.changed = changed;
    stored.from_cache = false;
    let json = serde_json::to_string(&stored)?;
    let host = doc.host();

    conn.execute(
        "INSERT INTO documents (crawl_id, url, url_hash, final_url, host, status, status_code, title,
            content_hash, tokens, changed, fetched_at, document_json)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            crawl_id,
            doc.url,
            hash,
            doc.final_url,
            host,
            doc.status.as_str(),
            doc.status_code,
            doc.metadata.title,
            doc.content_hash as i64,
            doc.tokens as i64,
            changed,
            doc.fetched_at as i64,
            json
        ],
    )?;
    let id = conn.last_insert_rowid();

    if doc.status == crate::extract::PageStatus::Ok {
        let mut insert = conn.prepare_cached(
            "INSERT INTO chunks (text, heading_path, url, host, crawl_id, document_id, selector)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )?;
        for chunk in chunk_blocks(&doc.blocks, DEFAULT_CHUNK_TOKENS) {
            insert.execute(params![
                chunk.text,
                chunk.heading_path.join(HEADING_SEPARATOR),
                doc.final_url,
                host,
                crawl_id.unwrap_or(""),
                id,
                chunk.selector
            ])?;
        }
    }

    Ok(StoredDocument { id, changed })
}

const HEADING_SEPARATOR: &str = " > ";

/// Returns the most recent successfully fetched copy of `url` and its fetch time (Unix seconds).
pub fn latest_document(conn: &Connection, url: &str) -> SeoResult<Option<(PageDocument, u64)>> {
    let hash = url_hash(url) as i64;
    let row: Option<(String, i64)> = conn
        .query_row(
            "SELECT document_json, fetched_at FROM documents
             WHERE url_hash = ?1 AND url = ?2 AND status = 'ok'
             ORDER BY fetched_at DESC, id DESC LIMIT 1",
            params![hash, url],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    match row {
        Some((json, fetched_at)) => {
            let doc: PageDocument = serde_json::from_str(&json)?;
            Ok(Some((doc, fetched_at.max(0) as u64)))
        }
        None => Ok(None),
    }
}

/// Returns a page of documents stored for a crawl, in fetch order, and the total count.
pub fn documents_for_crawl(
    conn: &Connection,
    crawl_id: &str,
    offset: usize,
    limit: usize,
) -> SeoResult<(Vec<PageDocument>, usize)> {
    let total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM documents WHERE crawl_id = ?1",
        [crawl_id],
        |r| r.get(0),
    )?;
    let mut stmt = conn.prepare(
        "SELECT document_json FROM documents WHERE crawl_id = ?1 ORDER BY id LIMIT ?2 OFFSET ?3",
    )?;
    let rows = stmt.query_map(params![crawl_id, limit as i64, offset as i64], |r| {
        r.get::<_, String>(0)
    })?;
    let mut docs = Vec::new();
    for row in rows {
        docs.push(serde_json::from_str(&row?)?);
    }
    Ok((docs, total.max(0) as usize))
}

/// Full-text query over stored passages.
#[derive(Debug, Clone, Default)]
pub struct ChunkQuery {
    /// Keywords or a natural-language question.
    pub query: String,
    /// Restrict to one crawl.
    pub crawl_id: Option<String>,
    /// Restrict to one host.
    pub host: Option<String>,
    /// Restrict to URLs starting with this prefix.
    pub url_prefix: Option<String>,
    /// Maximum results (default 5).
    pub top_k: usize,
}

/// A ranked passage.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChunkHit {
    /// Page URL.
    pub url: String,
    /// Headings above the passage.
    pub heading_path: Vec<String>,
    /// Passage text.
    pub text: String,
    /// Passage with matched terms wrapped in `**`.
    pub snippet: String,
    /// CSS selector of the passage's first block.
    pub selector: String,
    /// Relevance score (higher is better).
    pub score: f64,
}

/// Ranks stored passages with SQLite's `bm25()`, weighting heading matches twice as much as
/// body matches. Query terms are OR-ed so questions match passages holding only some words.
pub fn search_chunks(conn: &Connection, q: &ChunkQuery) -> SeoResult<Vec<ChunkHit>> {
    let terms = crate::extract::find::query_terms(&q.query, true);
    if terms.is_empty() {
        return Ok(Vec::new());
    }
    let match_expr = terms
        .iter()
        .map(|t| format!("\"{}\"", t.replace('"', "")))
        .collect::<Vec<_>>()
        .join(" OR ");
    let top_k = if q.top_k == 0 { 5 } else { q.top_k };

    let mut stmt = conn.prepare(
        "SELECT url, heading_path, text, snippet(chunks, 0, '**', '**', '…', 24), selector,
                bm25(chunks, 1.0, 2.0) AS rank
         FROM chunks
         WHERE chunks MATCH ?1
           AND (?2 IS NULL OR crawl_id = ?2)
           AND (?3 IS NULL OR host = ?3)
           AND (?4 IS NULL OR substr(url, 1, length(?4)) = ?4)
         ORDER BY rank LIMIT ?5",
    )?;
    let rows = stmt.query_map(
        params![match_expr, q.crawl_id, q.host, q.url_prefix, top_k as i64],
        |r| {
            let heading: String = r.get(1)?;
            let rank: f64 = r.get(5)?;
            Ok(ChunkHit {
                url: r.get(0)?,
                heading_path: if heading.is_empty() {
                    Vec::new()
                } else {
                    heading
                        .split(HEADING_SEPARATOR)
                        .map(str::to_string)
                        .collect()
                },
                text: r.get(2)?,
                snippet: r.get(3)?,
                selector: r.get(4)?,
                // bm25() is lower-is-better and negative; flip it so callers see higher-is-better.
                score: -rank,
            })
        },
    )?;
    let mut hits = Vec::new();
    for row in rows {
        hits.push(row?);
    }
    Ok(hits)
}

/// A background content crawl job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContentCrawlRecord {
    /// Job id.
    pub id: String,
    /// Seed URL.
    pub target_url: String,
    /// Crawl options as JSON.
    pub options_json: String,
    /// `queued`, `crawling`, `completed`, `cancelled` or `failed`.
    pub status: String,
    /// Pages stored with status `ok`.
    pub pages_done: u32,
    /// Pages that failed or were blocked.
    pub pages_failed: u32,
    /// URLs skipped (robots.txt, filters).
    pub pages_skipped: u32,
    /// Failure message.
    pub error: Option<String>,
    /// Unix seconds.
    pub created_at: u64,
    /// Unix seconds.
    pub finished_at: Option<u64>,
}

/// Registers a new content crawl job.
pub fn create_content_crawl(
    conn: &Connection,
    id: &str,
    target_url: &str,
    options_json: &str,
) -> SeoResult<()> {
    conn.execute(
        "INSERT INTO content_crawls (id, target_url, status, options_json, created_at)
         VALUES (?1, ?2, 'queued', ?3, ?4)",
        params![id, target_url, options_json, unix_now() as i64],
    )?;
    Ok(())
}

/// Updates a job's status and counters. Terminal statuses also set `finished_at`.
pub fn update_content_crawl(
    conn: &Connection,
    id: &str,
    status: &str,
    done: u32,
    failed: u32,
    skipped: u32,
    error: Option<&str>,
) -> SeoResult<()> {
    let finished =
        matches!(status, "completed" | "cancelled" | "failed").then(|| unix_now() as i64);
    let changed = conn.execute(
        "UPDATE content_crawls SET status = ?2, pages_done = ?3, pages_failed = ?4,
            pages_skipped = ?5, error = COALESCE(?6, error), finished_at = COALESCE(?7, finished_at)
         WHERE id = ?1",
        params![id, status, done, failed, skipped, error, finished],
    )?;
    if changed == 0 {
        return Err(SeoError::Storage(format!("Unknown content crawl '{id}'")));
    }
    Ok(())
}

/// Reads a content crawl job.
pub fn get_content_crawl(conn: &Connection, id: &str) -> SeoResult<Option<ContentCrawlRecord>> {
    Ok(conn
        .query_row(
            "SELECT id, target_url, status, pages_done, pages_failed, pages_skipped, error,
                    created_at, finished_at, options_json
             FROM content_crawls WHERE id = ?1",
            [id],
            |r| {
                Ok(ContentCrawlRecord {
                    id: r.get(0)?,
                    target_url: r.get(1)?,
                    status: r.get(2)?,
                    pages_done: r.get(3)?,
                    pages_failed: r.get(4)?,
                    pages_skipped: r.get(5)?,
                    error: r.get(6)?,
                    created_at: r.get::<_, i64>(7)?.max(0) as u64,
                    finished_at: r.get::<_, Option<i64>>(8)?.map(|v| v.max(0) as u64),
                    options_json: r.get(9)?,
                })
            },
        )
        .optional()?)
}

/// A stored CSS extraction rule for one field of one page template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredRule {
    /// Host the rule applies to.
    pub host: String,
    /// Template id (see [`crate::extract::learn::template_id`]).
    pub template_id: String,
    /// Field name.
    pub field: String,
    /// CSS selector, optionally with `@attr`.
    pub selector: String,
    /// Value type name (`text`, `price`, ...).
    pub value_type: String,
    /// `learned` or `caller`.
    pub source: String,
    /// Number of pages the rule was confirmed on.
    pub support: u32,
}

/// Inserts or replaces a rule.
pub fn upsert_rule(conn: &Connection, rule: &StoredRule) -> SeoResult<()> {
    conn.execute(
        "INSERT INTO extraction_rules (host, template_id, field, selector, value_type, source, support, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(host, template_id, field) DO UPDATE SET
            selector = excluded.selector, value_type = excluded.value_type,
            source = excluded.source, support = excluded.support, updated_at = excluded.updated_at",
        params![
            rule.host,
            rule.template_id,
            rule.field,
            rule.selector,
            rule.value_type,
            rule.source,
            rule.support,
            unix_now() as i64
        ],
    )?;
    Ok(())
}

/// Lists the rules stored for a host and template.
pub fn rules_for(conn: &Connection, host: &str, template_id: &str) -> SeoResult<Vec<StoredRule>> {
    let mut stmt = conn.prepare(
        "SELECT host, template_id, field, selector, value_type, source, support
         FROM extraction_rules WHERE host = ?1 AND template_id = ?2 ORDER BY field",
    )?;
    let rows = stmt.query_map(params![host, template_id], |r| {
        Ok(StoredRule {
            host: r.get(0)?,
            template_id: r.get(1)?,
            field: r.get(2)?,
            selector: r.get(3)?,
            value_type: r.get(4)?,
            source: r.get(5)?,
            support: r.get(6)?,
        })
    })?;
    let mut rules = Vec::new();
    for row in rows {
        rules.push(row?);
    }
    Ok(rules)
}
