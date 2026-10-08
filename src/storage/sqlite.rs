//! # SQLite Connection & Schema Management
//!
//! Configures SQLite WAL mode, foreign keys, cache size, busy timeouts,
//! and runs migrations from embedded SQL schema definitions.

use crate::core::branding::{
    DATA_DIR_NAME, DB_FILE_NAME, ENV_DB_PATH, LEGACY_DB_FILE_NAME, LEGACY_ENV_DB_PATH,
    LEGACY_LOCAL_DB_DIR_NAME, LOCAL_DB_DIR_NAME,
};
use crate::error::{SeoError, SeoResult};
use rusqlite::Connection;
use std::path::{Path, PathBuf};

/// Embedded authoritative SQLite DDL schema for the SEO audit tables (migration 1).
pub const SCHEMA: &str = include_str!("schema.sql");

/// Ordered schema migrations, applied by [`apply_schema`] according to `PRAGMA user_version`.
///
/// Migration 1 is the original SEO schema (idempotent `IF NOT EXISTS` DDL), so databases created
/// before versioning existed upgrade cleanly from `user_version = 0`.
pub const MIGRATIONS: &[(i64, &str)] = &[
    (1, SCHEMA),
    (2, include_str!("migrations/0002_documents.sql")),
    (3, include_str!("migrations/0003_extraction_rules.sql")),
];

/// Schema version a fully migrated database reports through `PRAGMA user_version`.
pub const LATEST_SCHEMA_VERSION: i64 = 3;

/// Returns the local database path in the current working directory: `.blacksparrow/blacksparrow.db` (or legacy `.seolens/seolens.db`).
pub fn local_db_path() -> PathBuf {
    let legacy = PathBuf::from(LEGACY_LOCAL_DB_DIR_NAME).join(LEGACY_DB_FILE_NAME);
    if legacy.exists() {
        return legacy;
    }
    PathBuf::from(LOCAL_DB_DIR_NAME).join(DB_FILE_NAME)
}

/// Returns the standard default path for the SQLite persistence database.
///
/// Resolution precedence:
/// 1. `BLACKSPARROW_DB_PATH` or legacy `SEOLENS_DB_PATH` environment variable.
/// 2. Local `./.blacksparrow/blacksparrow.db` or legacy `./.seolens/seolens.db` if existing.
/// 3. OS standard user data directory:
///    - Linux: `$XDG_DATA_HOME/blacksparrow/blacksparrow.db` (defaults to `~/.local/share/blacksparrow/blacksparrow.db`)
///    - macOS: `~/Library/Application Support/blacksparrow/blacksparrow.db`
///    - Windows: `%LOCALAPPDATA%\blacksparrow\blacksparrow.db`
/// 4. Fallback to `./.blacksparrow/blacksparrow.db` if the OS data directory cannot be determined.
pub fn default_db_path() -> PathBuf {
    // 1. Environment variable override (check ENV_DB_PATH, then legacy LEGACY_ENV_DB_PATH)
    if let Ok(env_path) = std::env::var(ENV_DB_PATH).or_else(|_| std::env::var(LEGACY_ENV_DB_PATH))
    {
        let trimmed = env_path.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }

    // 2. Existing local directory in current working directory
    let local = local_db_path();
    if local.exists() {
        return local;
    }

    // 3. Standard modern OS user data directory
    if let Some(mut data_dir) = dirs::data_dir() {
        let legacy_path = data_dir.join("seolens").join(LEGACY_DB_FILE_NAME);
        if legacy_path.exists() {
            return legacy_path;
        }
        data_dir.push(DATA_DIR_NAME);
        data_dir.push(DB_FILE_NAME);
        return data_dir;
    }

    // 4. Fallback
    local
}

/// Resolves the database path based on explicit CLI arguments, the local flag, and environment/OS defaults.
///
/// Precedence:
/// 1. Explicit path (`--db-path <PATH>`) if provided.
/// 2. `local` (`-L, --local`) flag if true: returns `./.blacksparrow/blacksparrow.db`.
/// 3. Standard resolution via [`default_db_path`]:
///    - `BLACKSPARROW_DB_PATH` / `SEOLENS_DB_PATH` environment variable
///    - Local `./.blacksparrow/blacksparrow.db` if it already exists
///    - Standard OS user data directory (`~/.local/share/blacksparrow/blacksparrow.db` on Linux, etc.)
///    - Fallback `./.blacksparrow/blacksparrow.db`
pub fn resolve_db_path(explicit: Option<PathBuf>, local: bool) -> PathBuf {
    if let Some(p) = explicit {
        return p;
    }
    if local {
        return local_db_path();
    }
    default_db_path()
}

/// Opens a configured SQLite connection to the specified path without re-executing schema DDL migrations.
pub fn connect_configured(path: &Path) -> SeoResult<Connection> {
    let conn = Connection::open(path)?;
    configure_connection(&conn)?;
    Ok(conn)
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

/// Applies high-throughput PRAGMAs for concurrency, caching, and durability without modifying schema.
pub fn configure_connection(conn: &Connection) -> SeoResult<()> {
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

    // 6. Temp Store: Keep temporary indices, sort buffers, and transient tables in RAM.
    conn.pragma_update(None, "temp_store", "MEMORY")?;

    // 7. Memory-mapped I/O: 256 MiB memory mapping for microsecond read latency.
    let _ = conn.pragma_update(None, "mmap_size", 268435456i64);

    Ok(())
}

/// Applies every migration newer than the database's `PRAGMA user_version`.
pub fn apply_schema(conn: &Connection) -> SeoResult<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    for &(version, sql) in MIGRATIONS {
        if version <= current {
            continue;
        }
        if version == 1 {
            // Migration 1 carries journal-mode PRAGMAs, which SQLite rejects inside a transaction.
            conn.execute_batch(sql)?;
            conn.pragma_update(None, "user_version", version)?;
        } else {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE;\n{sql}\nPRAGMA user_version = {version};\nCOMMIT;"
            ))
            .inspect_err(|_| {
                let _ = conn.execute_batch("ROLLBACK;");
            })?;
        }
    }
    Ok(())
}

/// Configures connection PRAGMAs for concurrency and durability, then executes schema migrations.
pub fn init_connection(conn: &Connection) -> SeoResult<()> {
    configure_connection(conn)?;
    apply_schema(conn)?;
    Ok(())
}
