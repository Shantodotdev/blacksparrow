//! Centralized branding configuration and metadata.
//!
//! Isolates user-facing brand names, binary aliases, User-Agent strings,
//! and storage directory paths so the underlying engine logic is decoupled
//! and reusable.

/// The primary binary name derived at compile-time from Cargo.toml.
pub const BINARY_NAME: &str = env!("CARGO_PKG_NAME");

/// The short terminal alias for the primary binary.
pub const BINARY_ALIAS: &str = "sparrow";

/// The human-readable application display name.
pub const APP_DISPLAY_NAME: &str = "Black Sparrow";

/// Letter-spaced application display name for terminal ASCII banners.
pub const APP_DISPLAY_SPACED: &str = "B L A C K   S P A R R O W";

/// One-line description of the tool.
pub const APP_TAGLINE: &str = "Local-First Technical SEO Audit Engine";

/// Current package version derived from Cargo.toml.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Default crawler User-Agent string.
pub const DEFAULT_USER_AGENT: &str = concat!("BlackSparrow/", env!("CARGO_PKG_VERSION"));

/// AI crawler / bot User-Agent string for MCP probes.
pub const MCP_BOT_USER_AGENT: &str = concat!("BlackSparrowBot/", env!("CARGO_PKG_VERSION"));

/// MCP server identifier string.
pub const MCP_SERVER_NAME: &str = env!("CARGO_PKG_NAME");

/// Directory name for local workspace storage (e.g. `.blacksparrow`).
pub const LOCAL_DB_DIR_NAME: &str = concat!(".", env!("CARGO_PKG_NAME"));

/// Database file name (e.g. `blacksparrow.db`).
pub const DB_FILE_NAME: &str = concat!(env!("CARGO_PKG_NAME"), ".db");

/// Application data sub-directory for OS user data (e.g. `blacksparrow`).
pub const DATA_DIR_NAME: &str = env!("CARGO_PKG_NAME");

/// Primary environment variable for overriding database path.
pub const ENV_DB_PATH: &str = "BLACKSPARROW_DB_PATH";

/// Legacy environment variable for backwards compatibility.
pub const LEGACY_ENV_DB_PATH: &str = "SEOLENS_DB_PATH";

/// Legacy local directory name for backwards compatibility.
pub const LEGACY_LOCAL_DB_DIR_NAME: &str = ".seolens";

/// Legacy database file name for backwards compatibility.
pub const LEGACY_DB_FILE_NAME: &str = "seolens.db";

/// Default User-Agent tokens for robots.txt matching.
pub const ROBOTS_USER_AGENT_TOKENS: &[&str] = &["BlackSparrow", "BlackSparrowBot"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_branding_constants_integrity() {
        assert_eq!(BINARY_NAME, "blacksparrow");
        assert_eq!(BINARY_ALIAS, "sparrow");
        assert_eq!(APP_DISPLAY_NAME, "Black Sparrow");
        assert_eq!(LOCAL_DB_DIR_NAME, ".blacksparrow");
        assert_eq!(DB_FILE_NAME, "blacksparrow.db");
        assert_eq!(DATA_DIR_NAME, "blacksparrow");
        assert!(DEFAULT_USER_AGENT.starts_with("BlackSparrow/"));
        assert!(MCP_BOT_USER_AGENT.starts_with("BlackSparrowBot/"));
        assert_eq!(MCP_SERVER_NAME, "blacksparrow");
    }
}
