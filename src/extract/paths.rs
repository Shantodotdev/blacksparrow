//! Include / exclude URL path filters for `map` and `crawl`.
//!
//! Patterns are matched against the URL path (plus `?query` when present):
//! - globs such as `/blog/**` or `/docs/*.html` (a missing leading `/` is added);
//! - regexes, written as `re:^/blog/` or detected automatically when the pattern uses regex
//!   syntax (`.*`, `^`, `$`, `\`, `(`, `|`, `+`), which keeps Firecrawl's `includePaths`
//!   regexes working. Regexes are unanchored unless they anchor themselves.

use crate::error::{SeoError, SeoResult};
use globset::{Glob, GlobSet, GlobSetBuilder};
use regex::Regex;

/// A compiled set of include and exclude patterns.
#[derive(Debug, Clone, Default)]
pub struct PathFilter {
    include: Patterns,
    exclude: Patterns,
}

#[derive(Debug, Clone, Default)]
struct Patterns {
    globs: Option<GlobSet>,
    regexes: Vec<Regex>,
}

impl Patterns {
    fn compile(patterns: &[String]) -> SeoResult<Self> {
        let mut builder = GlobSetBuilder::new();
        let mut has_globs = false;
        let mut regexes = Vec::new();
        for raw in patterns.iter().map(|p| p.trim()).filter(|p| !p.is_empty()) {
            if let Some(re) = raw.strip_prefix("re:") {
                regexes.push(compile_regex(re)?);
            } else if looks_like_regex(raw) {
                regexes.push(compile_regex(raw)?);
            } else {
                let pattern = if raw.starts_with('/') || raw.starts_with('*') {
                    raw.to_string()
                } else {
                    format!("/{raw}")
                };
                let glob = Glob::new(&pattern)
                    .map_err(|e| SeoError::Config(format!("Invalid path glob '{raw}': {e}")))?;
                builder.add(glob);
                has_globs = true;
            }
        }
        let globs = if has_globs {
            Some(
                builder
                    .build()
                    .map_err(|e| SeoError::Config(format!("Invalid path globs: {e}")))?,
            )
        } else {
            None
        };
        Ok(Self { globs, regexes })
    }

    fn is_empty(&self) -> bool {
        self.globs.is_none() && self.regexes.is_empty()
    }

    fn matches(&self, path: &str, path_and_query: &str) -> bool {
        self.globs
            .as_ref()
            .is_some_and(|g| g.is_match(path) || g.is_match(path_and_query))
            || self.regexes.iter().any(|r| r.is_match(path_and_query))
    }
}

fn compile_regex(pattern: &str) -> SeoResult<Regex> {
    Regex::new(pattern)
        .map_err(|e| SeoError::Config(format!("Invalid path regex '{pattern}': {e}")))
}

fn looks_like_regex(pattern: &str) -> bool {
    pattern.contains(".*")
        || pattern.contains(".+")
        || pattern.starts_with('^')
        || pattern.ends_with('$')
        || pattern.contains(['\\', '(', '|', '+'])
}

impl PathFilter {
    /// Compiles include and exclude patterns.
    ///
    /// # Errors
    ///
    /// Returns [`SeoError::Config`] for an invalid glob or regex.
    pub fn new(include: &[String], exclude: &[String]) -> SeoResult<Self> {
        Ok(Self {
            include: Patterns::compile(include)?,
            exclude: Patterns::compile(exclude)?,
        })
    }

    /// Whether `url` passes the filter: it matches an include pattern (or there are none) and
    /// no exclude pattern. Unparseable URLs are rejected.
    pub fn allows(&self, url: &str) -> bool {
        let Ok(parsed) = url::Url::parse(url) else {
            return false;
        };
        let path = parsed.path();
        let path_and_query = match parsed.query() {
            Some(q) => format!("{path}?{q}"),
            None => path.to_string(),
        };
        (self.include.is_empty() || self.include.matches(path, &path_and_query))
            && !self.exclude.matches(path, &path_and_query)
    }
}
