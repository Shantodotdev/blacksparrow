//! # SQLite Connection & Schema Management
//!
//! Configures SQLite WAL mode, foreign keys, cache size, busy timeouts,
//! and runs migrations from embedded SQL schema definitions.

use crate::error::{SeoError, SeoResult};
use rusqlite::Connection;
use std::path::{Path, PathBuf};

/// Embedded authoritative SQLite DDL schema.
pub const SCHEMA: &str = include_str!("schema.sql");

/// Returns the local database path in the current working directory: `.seolens/seolens.db`.
pub fn local_db_path() -> PathBuf {
    PathBuf::from(".seolens").join("seolens.db")
}

/// Returns the standard default path for the SQLite persistence database.
///
/// Resolution precedence:
/// 1. `SEOLENS_DB_PATH` environment variable (if set and non-empty).
/// 2. Local `./.seolens/seolens.db` in current working directory if it already exists.
/// 3. OS standard user data directory:
///    - Linux: `$XDG_DATA_HOME/seolens/seolens.db` (defaults to `~/.local/share/seolens/seolens.db`)
///    - macOS: `~/Library/Application Support/seolens/seolens.db`
///    - Windows: `%LOCALAPPDATA%\seolens\seolens.db`
/// 4. Fallback to `./.seolens/seolens.db` if the OS data directory cannot be determined.
pub fn default_db_path() -> PathBuf {
    // 1. Environment variable override
    if let Ok(env_path) = std::env::var("SEOLENS_DB_PATH") {
        let trimmed = env_path.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }

    // 2. Existing local .seolens/seolens.db in current working directory
    let local = local_db_path();
    if local.exists() {
        return local;
    }

    // 3. Standard modern OS user data directory (XDG on Linux, App Support on macOS, AppData on Windows)
    if let Some(mut data_dir) = dirs::data_dir() {
        data_dir.push("seolens");
        data_dir.push("seolens.db");
        return data_dir;
    }

    // 4. Fallback
    local
}

/// Resolves the database path based on explicit CLI arguments, the local flag, and environment/OS defaults.
///
/// Precedence:
/// 1. Explicit path (`--db-path <PATH>`) if provided.
/// 2. `local` (`-L, --local`) flag if true: returns `./.seolens/seolens.db`.
/// 3. Standard resolution via [`default_db_path`]:
///    - `SEOLENS_DB_PATH` environment variable
///    - Local `./.seolens/seolens.db` if it already exists
///    - Standard OS user data directory (`~/.local/share/seolens/seolens.db` on Linux, etc.)
///    - Fallback `./.seolens/seolens.db`
pub fn resolve_db_path(explicit: Option<PathBuf>, local: bool) -> PathBuf {
    if let Some(p) = explicit {
        return p;
    }
    if local {
        return local_db_path();
    }
    default_db_path()
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
