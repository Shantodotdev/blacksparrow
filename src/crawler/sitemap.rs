//! # Zero-Copy Streaming XML Sitemap Parser
//!
//! Event-driven XML parsing engine for standard sitemaps (`<urlset>`) and
//! hierarchical sitemap indexes (`<sitemapindex>`), with on-the-fly `.xml.gz`
//! decompression and `hreflang` alternate link extraction.
//!
//! ## Memory & Performance
//!
//! Large sitemaps frequently contain up to 50,000 URLs per file (the Google specification limit).
//! Constructing an in-memory XML DOM (e.g. via `roxmltree`) would allocate tens of megabytes of heap
//! and generate significant GC pressure.
//!
//! `quick-xml` processes bytes in an event-driven pull stream, tokenizing tags
//! and extracting `<loc>`, `<lastmod>`, and `<xhtml:link>` attributes with zero DOM overhead.
//!
//! ## Examples
//!
//! ```rust
//! use seo_lens::crawler::sitemap::{parse_sitemap, SitemapDocument};
//!
//! let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
//! <urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
//!   <url>
//!     <loc>https://example.com/page-1</loc>
//!     <priority>0.8</priority>
//!   </url>
//! </urlset>"#;
//!
//! match parse_sitemap(xml.as_bytes()).unwrap() {
//!     SitemapDocument::UrlSet(entries) => {
//!         assert_eq!(entries.len(), 1);
//!         assert_eq!(entries[0].loc, "https://example.com/page-1");
//!     }
//!     SitemapDocument::Index(_) => panic!("Expected UrlSet"),
//! }
//! ```

use crate::error::{SeoError, SeoResult};
use compact_str::CompactString;
use flate2::read::GzDecoder;
use quick_xml::events::Event;
use quick_xml::reader::Reader;
use std::borrow::Cow;
use std::io::Read;

/// An alternate language or regional URL declared in a `<xhtml:link>` tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SitemapAlternate {
    /// Language or locale tag (e.g. `"en"`, `"es"`, `"x-default"`).
    pub hreflang: CompactString,
    /// Canonical target URL for that locale.
    pub href: CompactString,
}

/// A parsed entry from a `<urlset>` sitemap document.
#[derive(Debug, Clone, PartialEq)]
pub struct SitemapEntry {
    /// Target location URL.
    pub loc: CompactString,
    /// Last modification timestamp (ISO 8601 string if present).
    pub lastmod: Option<CompactString>,
    /// Expected change frequency (`always`, `hourly`, `daily`, etc.).
    pub changefreq: Option<CompactString>,
    /// Search priority weighting ($0.0 \le p \le 1.0$).
    pub priority: Option<f32>,
    /// Alternate hreflang representations.
    pub alternates: Vec<SitemapAlternate>,
}

/// A child sitemap entry from a `<sitemapindex>` document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SitemapIndexEntry {
    /// Sub-sitemap location URL.
    pub loc: CompactString,
    /// Last modification timestamp of the sub-sitemap.
    pub lastmod: Option<CompactString>,
}

/// The document type returned by sitemap parsing.
#[derive(Debug, Clone, PartialEq)]
pub enum SitemapDocument {
    /// Standard URL set containing individual crawl targets.
    UrlSet(Vec<SitemapEntry>),
    /// Sitemap index referencing nested sub-sitemaps.
    Index(Vec<SitemapIndexEntry>),
}

/// Parses raw sitemap bytes, automatically decompressing gzip if needed.
///
/// Supports both `<urlset>` and `<sitemapindex>` root nodes.
///
/// # Errors
///
/// Returns [`SeoError::Internal`] if the XML is malformed, unrecognized, or if
/// gzip decompression encounters corrupted bytes.
pub fn parse_sitemap(raw_bytes: &[u8]) -> SeoResult<SitemapDocument> {
    // Check for Gzip magic header bytes (0x1F, 0x8B)
    let xml_bytes: Cow<[u8]> =
        if raw_bytes.len() >= 2 && raw_bytes[0] == 0x1f && raw_bytes[1] == 0x8b {
            let mut decoder = GzDecoder::new(raw_bytes);
            let mut decompressed = Vec::new();
            decoder.read_to_end(&mut decompressed).map_err(|e| {
                SeoError::Internal(format!("Sitemap gzip decompression failed: {e}"))
            })?;
            Cow::Owned(decompressed)
        } else {
            Cow::Borrowed(raw_bytes)
        };

    let mut reader = Reader::from_reader(xml_bytes.as_ref());
    reader.config_mut().trim_text(true);

    let mut buf = Vec::with_capacity(512);

    let mut is_index = false;
    let mut url_entries: Vec<SitemapEntry> = Vec::new();
    let mut index_entries: Vec<SitemapIndexEntry> = Vec::new();

    // Field-level state tracking
    let mut current_tag: Option<Vec<u8>> = None;
    let mut current_loc: Option<CompactString> = None;
    let mut current_lastmod: Option<CompactString> = None;
    let mut current_changefreq: Option<CompactString> = None;
    let mut current_priority: Option<f32> = None;
    let mut current_alternates: Vec<SitemapAlternate> = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let local = e.local_name();
                match local.as_ref() {
                    b"sitemapindex" => {
                        is_index = true;
                    }
                    b"urlset" => {
                        is_index = false;
                    }
                    b"url" | b"sitemap" => {
                        current_loc = None;
                        current_lastmod = None;
                        current_changefreq = None;
                        current_priority = None;
                        current_alternates.clear();
                    }
                    b"loc" | b"lastmod" | b"changefreq" | b"priority" => {
                        current_tag = Some(local.as_ref().to_vec());
                    }
                    _ => {}
                }
            }
            Ok(Event::Empty(ref e)) => {
                let local = e.local_name();
                if local.as_ref() == b"link" {
                    // Check for <xhtml:link rel="alternate" hreflang="..." href="..." />
                    let mut rel_opt = None;
                    let mut hreflang_opt = None;
                    let mut href_opt = None;

                    for attr in e.attributes() {
                        let attr = attr
                            .map_err(|err| SeoError::Internal(format!("XML attribute: {err}")))?;
                        let key = attr.key.local_name();
                        let val_str = std::str::from_utf8(&attr.value)
                            .map_err(|err| SeoError::Internal(format!("Invalid UTF-8: {err}")))?;

                        match key.as_ref() {
                            b"rel" => rel_opt = Some(val_str.to_string()),
                            b"hreflang" => hreflang_opt = Some(CompactString::new(val_str)),
                            b"href" => href_opt = Some(CompactString::new(val_str)),
                            _ => {}
                        }
                    }

                    if rel_opt.as_deref() == Some("alternate") {
                        if let (Some(hreflang), Some(href)) = (hreflang_opt, href_opt) {
                            current_alternates.push(SitemapAlternate { hreflang, href });
                        }
                    }
                }
            }
            Ok(Event::Text(ref e)) => {
                if let Some(ref tag) = current_tag {
                    let text = e
                        .unescape()
                        .map_err(|err| SeoError::Internal(format!("XML unescape: {err}")))?;
                    let text_str = text.trim();

                    if !text_str.is_empty() {
                        match tag.as_slice() {
                            b"loc" => current_loc = Some(CompactString::new(text_str)),
                            b"lastmod" => current_lastmod = Some(CompactString::new(text_str)),
                            b"changefreq" => {
                                current_changefreq = Some(CompactString::new(text_str))
                            }
                            b"priority" => {
                                if let Ok(p) = text_str.parse::<f32>() {
                                    current_priority = Some(p);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            Ok(Event::End(ref e)) => {
                let local = e.local_name();
                match local.as_ref() {
                    b"url" => {
                        if let Some(loc) = current_loc.take() {
                            url_entries.push(SitemapEntry {
                                loc,
                                lastmod: current_lastmod.take(),
                                changefreq: current_changefreq.take(),
                                priority: current_priority.take(),
                                alternates: std::mem::take(&mut current_alternates),
                            });
                        }
                    }
                    b"sitemap" => {
                        if let Some(loc) = current_loc.take() {
                            index_entries.push(SitemapIndexEntry {
                                loc,
                                lastmod: current_lastmod.take(),
                            });
                        }
                    }
                    b"loc" | b"lastmod" | b"changefreq" | b"priority" => {
                        current_tag = None;
                    }
                    _ => {}
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(SeoError::Internal(format!("Sitemap XML parse error: {e}"))),
            _ => {}
        }
        buf.clear();
    }

    if is_index {
        Ok(SitemapDocument::Index(index_entries))
    } else {
        Ok(SitemapDocument::UrlSet(url_entries))
    }
}
