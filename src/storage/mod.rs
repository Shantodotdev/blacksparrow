//! # SQLite Persistence Layer
//!
//! Provides Write-Ahead Logging (WAL) database storage, asynchronous real-time batch
//! writing, transactional integrity, and querying for crawl sessions, pages, and issues.

pub mod queries;
pub mod sqlite;
pub mod writer;

pub use queries::{
    get_crawl, get_crawl_issues, get_crawl_pages, init_crawl_session, list_crawls,
    update_crawl_status, CrawlSessionInit,
};
pub use sqlite::{default_db_path, open_connection, SCHEMA};
pub use writer::{spawn_db_writer, DbMessage, DbWriterHandle};

use crate::core::models::{CrawlSummary, IssueCategory, IssueFinding, PageReport, Severity};
use crate::error::SeoResult;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// High-level SQLite database manager.
#[derive(Debug, Clone)]
pub struct Database {
    path: PathBuf,
}

impl Database {
    /// Opens or creates the SQLite database at the specified path and initializes the schema.
    pub fn open(path: impl AsRef<Path>) -> SeoResult<Self> {
        let path_buf = path.as_ref().to_path_buf();
        let _ = sqlite::open_connection(&path_buf)?;
        Ok(Self { path: path_buf })
    }

    /// Returns the database file path on disk.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Opens a new configured connection to the database.
    pub fn connect(&self) -> SeoResult<rusqlite::Connection> {
        sqlite::open_connection(&self.path)
    }

    /// Creates and persists a new crawl session.
    pub fn init_crawl_session(&self, init: &CrawlSessionInit) -> SeoResult<()> {
        let conn = self.connect()?;
        queries::init_crawl_session(&conn, init)
    }

    /// Updates session status, page counts, defects, and health score.
    #[allow(clippy::too_many_arguments)]
    pub fn update_crawl_status(
        &self,
        session_id: &str,
        status: &str,
        finished_at: Option<&str>,
        total_pages: u32,
        error_count: u32,
        alert_count: u32,
        warning_count: u32,
        health_score: Option<u8>,
    ) -> SeoResult<()> {
        let conn = self.connect()?;
        queries::update_crawl_status(
            &conn,
            session_id,
            status,
            finished_at,
            total_pages,
            error_count,
            alert_count,
            warning_count,
            health_score,
        )
    }

    /// Lists historical crawl sessions ordered by start time.
    pub fn list_crawls(&self) -> SeoResult<Vec<CrawlSummary>> {
        let conn = self.connect()?;
        queries::list_crawls(&conn)
    }

    /// Retrieves summary metadata for a single session.
    pub fn get_crawl(&self, session_id: &str) -> SeoResult<Option<CrawlSummary>> {
        let conn = self.connect()?;
        queries::get_crawl(&conn, session_id)
    }

    /// Queries paginated pages for a session with reconstructed child collections.
    pub fn get_crawl_pages(
        &self,
        session_id: &str,
        limit: usize,
        offset: usize,
    ) -> SeoResult<Vec<PageReport>> {
        let conn = self.connect()?;
        queries::get_crawl_pages(&conn, session_id, limit, offset)
    }

    /// Queries issues matching session ID with optional severity and category filters.
    pub fn get_crawl_issues(
        &self,
        session_id: &str,
        severity_filter: Option<Severity>,
        category_filter: Option<IssueCategory>,
    ) -> SeoResult<Vec<IssueFinding>> {
        let conn = self.connect()?;
        queries::get_crawl_issues(&conn, session_id, severity_filter, category_filter)
    }

    /// Spawns the real-time background writer actor task.
    pub fn spawn_writer(
        &self,
        session_id: &str,
        batch_size: usize,
        flush_interval: Duration,
    ) -> SeoResult<(DbWriterHandle, tokio::task::JoinHandle<SeoResult<()>>)> {
        writer::spawn_db_writer(
            self.path.clone(),
            session_id.to_string(),
            batch_size,
            flush_interval,
        )
    }
}
