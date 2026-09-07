//! # SQLite Persistence Layer
//!
//! Provides Write-Ahead Logging (WAL) database storage, asynchronous real-time batch
//! writing, transactional integrity, and querying for crawl sessions, pages, and issues.

pub mod queries;
pub mod sqlite;
pub mod writer;

pub use queries::{
    clean_all_crawls, clean_crawls_older_than, count_issues_filtered, delete_crawl, get_crawl,
    get_crawl_issues, get_crawl_pages, init_crawl_session, list_crawls, query_issues_filtered,
    update_crawl_status, CrawlSessionInit, IssueFilterCriteria,
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

    /// Creates a Database handle pointing to the specified path without opening a connection immediately.
    pub fn from_path(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
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

    /// Alias for initializing a new crawl session.
    pub fn init_crawl(&self, init: &CrawlSessionInit) -> SeoResult<()> {
        self.init_crawl_session(init)
    }

    /// Deletes a specific crawl session and its cascading child records.
    pub fn delete_crawl(&self, session_id: &str) -> SeoResult<bool> {
        let conn = self.connect()?;
        queries::delete_crawl(&conn, session_id)
    }

    /// Purges crawl sessions older than `days` days.
    pub fn clean_crawls_older_than(&self, days: u32) -> SeoResult<usize> {
        let conn = self.connect()?;
        queries::clean_crawls_older_than(&conn, days)
    }

    /// Purges all historical crawl sessions from the database.
    pub fn clean_all_crawls(&self) -> SeoResult<usize> {
        let conn = self.connect()?;
        queries::clean_all_crawls(&conn)
    }

    /// Queries issues with advanced filters (severity, category, code, url substring, and pagination).
    pub fn query_issues_filtered(
        &self,
        session_id: &str,
        criteria: &IssueFilterCriteria,
    ) -> SeoResult<Vec<IssueFinding>> {
        let conn = self.connect()?;
        queries::query_issues_filtered(&conn, session_id, criteria)
    }

    /// Counts issues matching the specified filters.
    pub fn count_issues_filtered(
        &self,
        session_id: &str,
        criteria: &IssueFilterCriteria,
    ) -> SeoResult<usize> {
        let conn = self.connect()?;
        queries::count_issues_filtered(&conn, session_id, criteria)
    }

    /// Directly persists a batch of page reports into SQLite within a transaction.
    pub fn save_page_batch(&self, session_id: &str, pages: &[PageReport]) -> SeoResult<()> {
        let mut conn = self.connect()?;
        let mut pages_vec = pages.to_vec();
        let mut issues_vec = Vec::new();
        writer::flush_to_db(&mut conn, session_id, &mut pages_vec, &mut issues_vec)
    }

    /// Directly persists a batch of issue findings into SQLite within a transaction.
    pub fn save_issue_batch(&self, session_id: &str, issues: &[IssueFinding]) -> SeoResult<()> {
        let mut conn = self.connect()?;
        let mut pages_vec = Vec::new();
        let mut issues_vec = issues.to_vec();
        writer::flush_to_db(&mut conn, session_id, &mut pages_vec, &mut issues_vec)
    }
}
