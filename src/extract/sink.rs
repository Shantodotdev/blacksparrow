//! Where crawled documents go: memory, NDJSON or a directory of Markdown files.
//!
//! Storage in SQLite is handled by the [`Scraper`](crate::extract::scrape::Scraper) itself
//! when it has a database, so every sink here can be combined with it.

use crate::error::SeoResult;
use crate::extract::markdown::to_markdown_file;
use crate::extract::types::{PageDocument, PageStatus};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Receives documents as a crawl produces them.
pub trait PageSink: Send {
    /// Handles one document.
    ///
    /// # Errors
    ///
    /// Returns an error when the document cannot be written; the crawl stops.
    fn write(&mut self, doc: &PageDocument) -> SeoResult<()>;

    /// Flushes buffered output at the end of a crawl.
    ///
    /// # Errors
    ///
    /// Returns an error when flushing fails.
    fn finish(&mut self) -> SeoResult<()> {
        Ok(())
    }
}

/// Keeps every document in memory.
#[derive(Debug, Default)]
pub struct MemorySink {
    /// Documents in the order they were written.
    pub docs: Vec<PageDocument>,
}

impl PageSink for MemorySink {
    fn write(&mut self, doc: &PageDocument) -> SeoResult<()> {
        self.docs.push(doc.clone());
        Ok(())
    }
}

/// Appends documents to a shared vector (used by background jobs so status reads can see
/// results while the crawl runs).
#[derive(Debug, Clone, Default)]
pub struct SharedSink {
    /// The shared documents.
    pub docs: Arc<Mutex<Vec<PageDocument>>>,
}

impl PageSink for SharedSink {
    fn write(&mut self, doc: &PageDocument) -> SeoResult<()> {
        if let Ok(mut docs) = self.docs.lock() {
            docs.push(doc.clone());
        }
        Ok(())
    }
}

/// Writes one JSON document per line.
pub struct NdjsonSink<W: Write + Send> {
    out: W,
}

impl<W: Write + Send> NdjsonSink<W> {
    /// Wraps a writer.
    pub fn new(out: W) -> Self {
        Self { out }
    }
}

impl<W: Write + Send> PageSink for NdjsonSink<W> {
    fn write(&mut self, doc: &PageDocument) -> SeoResult<()> {
        serde_json::to_writer(&mut self.out, doc)?;
        self.out.write_all(b"\n")?;
        Ok(())
    }

    fn finish(&mut self) -> SeoResult<()> {
        self.out.flush()?;
        Ok(())
    }
}

/// Writes each successful page as a Markdown file with frontmatter, mirroring the URL:
/// `https://host/docs/intro` becomes `<root>/host/docs/intro.md`, `/` becomes `index.md`.
#[derive(Debug, Clone)]
pub struct DirSink {
    root: PathBuf,
    /// Files written so far.
    pub written: Vec<PathBuf>,
}

impl DirSink {
    /// Creates the output directory if needed.
    ///
    /// # Errors
    ///
    /// Returns an error when the directory cannot be created.
    pub fn new(root: &Path) -> SeoResult<Self> {
        std::fs::create_dir_all(root)?;
        Ok(Self {
            root: root.to_path_buf(),
            written: Vec::new(),
        })
    }
}

impl PageSink for DirSink {
    fn write(&mut self, doc: &PageDocument) -> SeoResult<()> {
        if doc.status != PageStatus::Ok {
            return Ok(());
        }
        let path = self.root.join(relative_path_for(&doc.final_url));
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, to_markdown_file(doc))?;
        self.written.push(path);
        Ok(())
    }
}

/// File path (relative) for a URL: host directory, sanitised path segments and `.md`.
/// Query strings become a short hash suffix so distinct URLs never collide.
pub fn relative_path_for(url: &str) -> PathBuf {
    let Ok(parsed) = url::Url::parse(url) else {
        return PathBuf::from(format!("{}.md", sanitize(url)));
    };
    let mut host = parsed.host_str().unwrap_or("unknown").to_ascii_lowercase();
    if let Some(port) = parsed.port() {
        host = format!("{host}_{port}");
    }
    let mut path = PathBuf::from(sanitize(&host));
    let segments: Vec<String> = parsed
        .path_segments()
        .map(|s| s.filter(|seg| !seg.is_empty()).map(sanitize).collect())
        .unwrap_or_default();
    let trailing_slash = parsed.path().ends_with('/');
    let (dirs, file) = match segments.split_last() {
        Some((last, dirs)) if !trailing_slash => (dirs.to_vec(), last.clone()),
        _ => (segments.clone(), "index".to_string()),
    };
    for dir in dirs {
        path.push(dir);
    }
    let stem = file
        .strip_suffix(".html")
        .or_else(|| file.strip_suffix(".htm"))
        .unwrap_or(&file)
        .to_string();
    let stem = match parsed.query() {
        Some(q) => format!("{stem}__{:08x}", crate::core::url::url_hash(q) as u32),
        None => stem,
    };
    path.push(format!("{stem}.md"));
    path
}

fn sanitize(segment: &str) -> String {
    let cleaned: String = segment
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('.');
    if trimmed.is_empty() {
        "_".to_string()
    } else {
        trimmed.chars().take(120).collect()
    }
}
