//! # SQLite Connection & Schema Management
//!
//! Configures SQLite WAL mode, foreign keys, cache size, busy timeouts,
//! and runs migrations from embedded SQL schema definitions.

use crate::error::{SeoError, SeoResult};
use rusqlite::Connection;
use std::path::{Path, PathBuf};

/// Embedded authoritative SQLite DDL schema.
pub const SCHEMA: &str = include_str!("schema.sql");

/// Returns the standard default path for the SQLite database: `.seolens/seolens.db`.
pub fn default_db_path() -> PathBuf {
    PathBuf::from(".seolens").join("seolens.db")
}

/// Opens a SQLite connection to the specified path and applies WAL mode and schema.
pub fn open_connection(path: &Path) -> SeoResult<Connection> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| {
                SeoError::Storage(format!(
                    "Failed to create database directory '{}': {}",
                    parent.display(),
                    e
                ))
            })?;
        }
    }

    let conn = Connection::open(path)?;
    init_connection(&conn)?;
    Ok(conn)
}

/// Configures connection PRAGMAs for concurrency and durability, then executes schema migrations.
pub fn init_connection(conn: &Connection) -> SeoResult<()> {
    // 1. WAL mode: Non-blocking readers during background crawl ingestion via sequential log append.
    let _ = conn.pragma_update(None, "journal_mode", "WAL");

    // 2. Synchronous NORMAL: Avoids per-commit fsync stalls while preserving crash safety in WAL mode.
    conn.pragma_update(None, "synchronous", "NORMAL")?;

    // 3. Foreign Keys: Explicitly enforce ON DELETE CASCADE constraints (disabled by default in SQLite).
    conn.pragma_update(None, "foreign_keys", "ON")?;

    // 4. Busy Timeout: Sleep and retry up to 5,000ms to resolve transient lock contention gracefully.
    conn.pragma_update(None, "busy_timeout", 5000)?;

    // 5. Cache Size: Negative value allocates exactly 64 MiB (64,000 KiB) of RAM for B-Tree page cache.
    conn.pragma_update(None, "cache_size", -64000)?;

    // Execute schema DDL
    conn.execute_batch(SCHEMA)?;
    Ok(())
}
