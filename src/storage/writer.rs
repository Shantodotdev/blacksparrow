//! # Asynchronous Real-Time Batch Writer Actor
//!
//! Streams crawled pages, links, structured data, and audit findings from Tokio green tasks
//! into SQLite without blocking worker threads. Employs double-trigger batch transactions
//! (threshold and interval timers) with transactional integrity.

use crate::core::models::{IssueFinding, PageReport};
use crate::error::{SeoError, SeoResult};
use crate::storage::queries::update_crawl_status;
use crate::storage::sqlite::open_connection;
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

/// Messages processed by the background SQLite writer actor.
pub enum DbMessage {
    /// Save a crawled page report with all child links, images, schemas, and findings.
    Page(Box<PageReport>),
    /// Save site-wide or graph issues discovered during or after the crawl.
    Issues(Vec<IssueFinding>),
    /// Update session final status, counts, and health score.
    UpdateStatus {
        status: String,
        finished_at: Option<String>,
        total_pages: u32,
        errors: u32,
        alerts: u32,
        warnings: u32,
        health_score: Option<u8>,
    },
    /// Flush any pending buffered records immediately.
    Flush(oneshot::Sender<()>),
    /// Flush pending records and shut down the writer actor.
    Shutdown(oneshot::Sender<()>),
}

/// Handle held by callers to stream audit records into SQLite.
#[derive(Clone)]
pub struct DbWriterHandle {
    tx: mpsc::Sender<DbMessage>,
}

impl DbWriterHandle {
    /// Dispatches a page report to the background writer queue.
    pub async fn save_page(&self, page: PageReport) -> SeoResult<()> {
        self.tx
            .send(DbMessage::Page(Box::new(page)))
            .await
            .map_err(|_| SeoError::Storage("Writer actor queue closed".into()))
    }

    /// Dispatches site-wide or graph issues to the writer queue.
    pub async fn save_issues(&self, issues: Vec<IssueFinding>) -> SeoResult<()> {
        self.tx
            .send(DbMessage::Issues(issues))
            .await
            .map_err(|_| SeoError::Storage("Writer actor queue closed".into()))
    }

    /// Updates crawl session status, counts, and health score.
    #[allow(clippy::too_many_arguments)]
    pub async fn update_status(
        &self,
        status: String,
        finished_at: Option<String>,
        total_pages: u32,
        errors: u32,
        alerts: u32,
        warnings: u32,
        health_score: Option<u8>,
    ) -> SeoResult<()> {
        self.tx
            .send(DbMessage::UpdateStatus {
                status,
                finished_at,
                total_pages,
                errors,
                alerts,
                warnings,
                health_score,
            })
            .await
            .map_err(|_| SeoError::Storage("Writer actor queue closed".into()))
    }

    /// Flushes all pending buffered writes to disk and awaits confirmation.
    pub async fn flush(&self) -> SeoResult<()> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.tx
            .send(DbMessage::Flush(resp_tx))
            .await
            .map_err(|_| SeoError::Storage("Writer actor queue closed".into()))?;
        resp_rx
            .await
            .map_err(|_| SeoError::Storage("Writer flush response dropped".into()))
    }

    /// Flushes pending writes and shuts down the background writer task.
    pub async fn shutdown(&self) -> SeoResult<()> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.tx
            .send(DbMessage::Shutdown(resp_tx))
            .await
            .map_err(|_| SeoError::Storage("Writer actor queue closed".into()))?;
        resp_rx
            .await
            .map_err(|_| SeoError::Storage("Writer shutdown response dropped".into()))
    }
}

/// Spawns the background writer actor task.
pub fn spawn_db_writer(
    db_path: PathBuf,
    session_id: String,
    batch_size: usize,
    flush_interval: Duration,
) -> SeoResult<(DbWriterHandle, tokio::task::JoinHandle<SeoResult<()>>)> {
    let (tx, rx) = mpsc::channel(1024);
    let handle = DbWriterHandle { tx };

    let join_handle = tokio::spawn(async move {
        run_writer_loop(&db_path, &session_id, rx, batch_size, flush_interval).await
    });

    Ok((handle, join_handle))
}

async fn run_writer_loop(
    db_path: &Path,
    session_id: &str,
    mut rx: mpsc::Receiver<DbMessage>,
    batch_size: usize,
    flush_interval: Duration,
) -> SeoResult<()> {
    let mut conn = open_connection(db_path)?;
    let mut buffer: Vec<PageReport> = Vec::with_capacity(batch_size);
    let mut pending_issues: Vec<IssueFinding> = Vec::new();
    let mut ticker = tokio::time::interval(flush_interval);

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                if !buffer.is_empty() || !pending_issues.is_empty() {
                    flush_to_db(&mut conn, session_id, &mut buffer, &mut pending_issues)?;
                }
            }
            msg = rx.recv() => {
                match msg {
                    Some(DbMessage::Page(page)) => {
                        buffer.push(*page);
                        if buffer.len() >= batch_size {
                            flush_to_db(&mut conn, session_id, &mut buffer, &mut pending_issues)?;
                        }
                    }
                    Some(DbMessage::Issues(issues)) => {
                        pending_issues.extend(issues);
                    }
                    Some(DbMessage::UpdateStatus {
                        status,
                        finished_at,
                        total_pages,
                        errors,
                        alerts,
                        warnings,
                        health_score,
                    }) => {
                        flush_to_db(&mut conn, session_id, &mut buffer, &mut pending_issues)?;
                        update_crawl_status(
                            &conn,
                            session_id,
                            &status,
                            finished_at.as_deref(),
                            total_pages,
                            errors,
                            alerts,
                            warnings,
                            health_score,
                        )?;
                    }
                    Some(DbMessage::Flush(responder)) => {
                        flush_to_db(&mut conn, session_id, &mut buffer, &mut pending_issues)?;
                        let _ = responder.send(());
                    }
                    Some(DbMessage::Shutdown(responder)) => {
                        flush_to_db(&mut conn, session_id, &mut buffer, &mut pending_issues)?;
                        let _ = responder.send(());
                        break;
                    }
                    None => {
                        flush_to_db(&mut conn, session_id, &mut buffer, &mut pending_issues)?;
                        break;
                    }
                }
            }
        }
    }

    Ok(())
}

fn flush_to_db(
    conn: &mut Connection,
    crawl_id: &str,
    pages: &mut Vec<PageReport>,
    issues: &mut Vec<IssueFinding>,
) -> SeoResult<()> {
    if pages.is_empty() && issues.is_empty() {
        return Ok(());
    }

    let tx = conn.transaction()?;

    if !pages.is_empty() {
        let mut page_stmt = tx.prepare_cached(
            "INSERT INTO pages (
                crawl_id, url, url_hash, final_url, status_code, content_type,
                size_bytes, ttfb_ms, crawl_depth, title, title_length, meta_description,
                meta_desc_length, canonical_url, html_lang, charset, viewport,
                robots_flags, is_sitemap_url, is_internal, h1_primary, h1_count,
                h2_headings, h3_headings, word_count, content_hash, simhash,
                is_soft_404, has_lorem_ipsum, is_https, has_hsts, has_csp,
                has_x_frame, has_x_content_type, mixed_content_count, page_intent
            ) VALUES (
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15,
                ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27, ?28,
                ?29, ?30, ?31, ?32, ?33, ?34, ?35, ?36
            )",
        )?;

        let mut link_stmt = tx.prepare_cached(
            "INSERT INTO links (
                crawl_id, source_url, target_url, target_url_hash, anchor_text,
                is_internal, is_nofollow, is_image_link, is_target_blank,
                has_opener_or_referrer, status_code
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        )?;

        let mut img_stmt = tx.prepare_cached(
            "INSERT INTO images (
                crawl_id, page_url, src_url, alt_text, width, height, size_bytes,
                has_dimensions, is_broken
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )?;

        let mut schema_stmt = tx.prepare_cached(
            "INSERT INTO schemas (
                crawl_id, page_url, schema_type, raw_json, is_valid_json, is_google_eligible
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;

        let mut href_stmt = tx.prepare_cached(
            "INSERT INTO hreflangs (
                crawl_id, page_url, lang_code, target_url, is_reciprocal
            ) VALUES (?1, ?2, ?3, ?4, ?5)",
        )?;

        let mut issue_stmt = tx.prepare_cached(
            "INSERT INTO issues (
                crawl_id, target_url, code, category, severity, title, message, source_page_url
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )?;

        for page in pages.drain(..) {
            let h2_json = serde_json::to_string(&page.h2_headings).unwrap_or_else(|_| "[]".into());
            let h3_json = serde_json::to_string(&page.h3_headings).unwrap_or_else(|_| "[]".into());
            let intent_json =
                serde_json::to_string(&page.page_intent).unwrap_or_else(|_| "{}".into());

            page_stmt.execute(params![
                crawl_id,
                page.url,
                page.url_hash as i64,
                page.final_url,
                page.status_code,
                page.content_type.as_str(),
                page.size_bytes,
                page.ttfb_ms,
                page.crawl_depth,
                page.title,
                page.title_length,
                page.meta_description,
                page.meta_desc_length,
                page.canonical_url,
                page.html_lang.as_deref(),
                page.charset.as_deref(),
                page.viewport.as_deref(),
                page.robots_flags.bits(),
                page.is_sitemap_url,
                page.is_internal,
                page.h1_primary,
                page.h1_count,
                h2_json,
                h3_json,
                page.word_count,
                page.content_hash as i64,
                page.simhash as i64,
                page.is_soft_404,
                page.has_lorem_ipsum,
                page.is_https,
                page.has_hsts,
                page.has_csp,
                page.has_x_frame,
                page.has_x_content_type,
                page.mixed_content_count,
                intent_json,
            ])?;

            for link in page.links {
                link_stmt.execute(params![
                    crawl_id,
                    link.source_url,
                    link.target_url,
                    link.target_url_hash as i64,
                    link.anchor_text,
                    link.is_internal,
                    link.is_nofollow,
                    link.is_image_link,
                    link.is_target_blank,
                    link.has_opener_or_referrer,
                    link.status_code,
                ])?;
            }

            for img in page.images {
                img_stmt.execute(params![
                    crawl_id,
                    page.url,
                    img.src_url,
                    img.alt_text,
                    img.width,
                    img.height,
                    img.size_bytes,
                    img.has_dimensions,
                    img.is_broken,
                ])?;
            }

            for schema in page.schemas {
                schema_stmt.execute(params![
                    crawl_id,
                    page.url,
                    schema.schema_type.as_str(),
                    schema.raw_json,
                    schema.is_valid_json,
                    schema.is_google_eligible,
                ])?;
            }

            for href in page.hreflangs {
                href_stmt.execute(params![
                    crawl_id,
                    page.url,
                    href.lang_code.as_str(),
                    href.target_url,
                    href.is_reciprocal,
                ])?;
            }

            for issue in page.issues {
                issue_stmt.execute(params![
                    crawl_id,
                    issue.target_url,
                    issue.code.as_str(),
                    issue.category.as_str(),
                    issue.severity.as_u8(),
                    issue.title.as_str(),
                    issue.message,
                    issue.source_page_url,
                ])?;
            }
        }
    }

    if !issues.is_empty() {
        let mut issue_stmt = tx.prepare_cached(
            "INSERT INTO issues (
                crawl_id, target_url, code, category, severity, title, message, source_page_url
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )?;

        for issue in issues.drain(..) {
            issue_stmt.execute(params![
                crawl_id,
                issue.target_url,
                issue.code.as_str(),
                issue.category.as_str(),
                issue.severity.as_u8(),
                issue.title.as_str(),
                issue.message,
                issue.source_page_url,
            ])?;
        }
    }

    tx.commit()?;
    Ok(())
}
