//! # Streaming HTML Parser Engine (`lol_html`)
//!
//! Zero-copy streaming HTML tokenizer powered by Cloudflare's [`lol_html`].
//!
//! ## Streaming Architecture
//!
//! Traditional HTML parsers construct a complete DOM tree in heap memory. For a crawler
//! inspecting thousands of pages concurrently, memory consumption quickly balloons to several
//! gigabytes.
//!
//! `SEO Lens` executes streaming tokenization using pre-compiled CSS selectors directly over
//! the incoming byte chunks:
//!
//! ```text
//! Incoming HTML Chunks
//!          │
//!          ▼
//! ┌────────────────────────────────────────────────────────┐
//! │ lol_html::HtmlRewriter Engine                          │
//! │                                                        │
//! │ ├─ Element Handlers: html, meta, link, h1-h3, a, img    │
//! │ ├─ Text Handlers: title, script[ld+json], body text    │
//! │ └─ Exclusion State: nav, header, footer, script, style  │
//! └────────────────────────┬───────────────────────────────┘
//!                          │
//!                          ▼
//!                    ParsedPage
//! ```
//!
//! ## Editorial Text Isolation State Machine
//!
//! To accurately calculate word counts and SimHash fingerprints without boilerplate noise
//! (navigation menus, cookie banners, headers, footers), the parser tracks an `exclusion_depth`
//! state counter:
//!
//! - Entering `<header>`, `<nav>`, `<footer>`, `<script>`, `<style>`, `<noscript>`, or `<svg>`
//!   increments `exclusion_depth`.
//! - Exiting those elements decrements `exclusion_depth`.
//! - Any text token emitted while `exclusion_depth == 0` is recognized as editorial content.
//!
//! ## Hyperlink Resolution Pipeline
//!
//! Anchor tags (`<a href="...">`) capture their associated anchor text (or child image `alt`
//! indicators) until the closing `</a>` tag is encountered. Targets are normalized, validated,
//! and resolved against the document's `base_url`. Non-HTTP targets (`javascript:`, `mailto:`,
//! `#hash`, `tel:`) are skipped.

use crate::core::models::{DiscoveredLink, HreflangTag, ImageResource, RobotsFlags, SchemaRecord};
use crate::core::url::{is_internal, resolve_relative, url_hash};
use crate::error::{SeoError, SeoResult};
use crate::parser::content::{compute_content_hash, compute_simhash, count_words};
use crate::parser::metadata::{clean_whitespace, decode_html_entities, parse_robots_directives};
use crate::parser::schema::parse_json_ld;
use crate::parser::ParsedPage;
use compact_str::CompactString;
use lol_html::{element, end_tag, text, HtmlRewriter, Settings};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Default)]
struct PendingLink {
    href: String,
    is_nofollow: bool,
    anchor_text: String,
    is_image_link: bool,
}

/// Parses an HTML document string against a base URL using `lol_html`.
///
/// Extracts all technical SEO metadata, heading hierarchy, hyperlinks,
/// image assets, JSON-LD schemas, and editorial content metrics in a single pass.
///
/// # Arguments
///
/// * `html` - Raw HTML string to parse.
/// * `base_url` - The base URL of the document, used to resolve relative URLs.
///
/// # Returns
///
/// Returns a fully populated [`ParsedPage`] containing all extracted structures.
///
/// # Errors
///
/// Returns [`SeoError::Internal`] if the underlying `lol_html` rewriter encounters
/// an unrecoverable tokenization error.
///
/// # Examples
///
/// ```rust
/// use seo_lens::parser::parse_html;
///
/// let html = r#"
///     <!DOCTYPE html>
///     <html lang="en">
///     <head>
///         <title>SEO Lens Quickstart</title>
///         <link rel="canonical" href="https://example.com/docs">
///     </head>
///     <body>
///         <h1>Documentation</h1>
///         <p>Learn how to audit websites efficiently.</p>
///         <a href="/guides">Guides</a>
///     </body>
///     </html>
/// "#;
///
/// let page = parse_html(html, "https://example.com/docs").unwrap();
/// assert_eq!(page.title.as_deref(), Some("SEO Lens Quickstart"));
/// assert_eq!(page.h1_primary.as_deref(), Some("Documentation"));
/// assert_eq!(page.links.len(), 1);
/// assert_eq!(page.links[0].target_url, "https://example.com/guides");
/// ```
pub fn parse_html(html: &str, base_url: &str) -> SeoResult<ParsedPage> {
    let title_buf = Rc::new(RefCell::new(String::new()));
    let meta_desc = Rc::new(RefCell::new(None::<String>));
    let canonical = Rc::new(RefCell::new(None::<String>));
    let html_lang = Rc::new(RefCell::new(None::<CompactString>));
    let charset = Rc::new(RefCell::new(None::<CompactString>));
    let viewport = Rc::new(RefCell::new(None::<CompactString>));
    let robots_flags = Rc::new(RefCell::new(RobotsFlags::NONE));

    let h1_list = Rc::new(RefCell::new(Vec::<String>::new()));
    let current_h1 = Rc::new(RefCell::new(None::<String>));

    let h2_list = Rc::new(RefCell::new(Vec::<String>::new()));
    let current_h2 = Rc::new(RefCell::new(None::<String>));

    let h3_list = Rc::new(RefCell::new(Vec::<String>::new()));
    let current_h3 = Rc::new(RefCell::new(None::<String>));

    let links = Rc::new(RefCell::new(Vec::<DiscoveredLink>::new()));
    let current_link = Rc::new(RefCell::new(None::<PendingLink>));

    let images = Rc::new(RefCell::new(Vec::<ImageResource>::new()));
    let schemas = Rc::new(RefCell::new(Vec::<SchemaRecord>::new()));
    let json_ld_buf = Rc::new(RefCell::new(String::new()));

    let hreflangs = Rc::new(RefCell::new(Vec::<HreflangTag>::new()));
    let open_graph = Rc::new(RefCell::new(Vec::<(CompactString, String)>::new()));
    let twitter_cards = Rc::new(RefCell::new(Vec::<(CompactString, String)>::new()));

    let exclusion_depth = Rc::new(RefCell::new(0usize));
    let editorial_text = Rc::new(RefCell::new(String::new()));

    let mut settings = Settings::new();

    // 1. HTML tag
    {
        let lang_ref = Rc::clone(&html_lang);
        settings = settings.append_element_content_handler(element!("html", move |el| {
            if let Some(lang) = el.get_attribute("lang") {
                *lang_ref.borrow_mut() = Some(CompactString::new(lang.trim()));
            }
            Ok(())
        }));
    }

    // 2. Title tag
    {
        let t_ref = Rc::clone(&title_buf);
        settings = settings.append_element_content_handler(text!("title", move |t| {
            t_ref.borrow_mut().push_str(t.as_str());
            Ok(())
        }));
    }

    // 3. Meta tags
    {
        let desc_ref = Rc::clone(&meta_desc);
        let robots_ref = Rc::clone(&robots_flags);
        let viewport_ref = Rc::clone(&viewport);
        let charset_ref = Rc::clone(&charset);
        let og_ref = Rc::clone(&open_graph);
        let tw_ref = Rc::clone(&twitter_cards);

        settings = settings.append_element_content_handler(element!("meta", move |el| {
            if let Some(name) = el.get_attribute("name") {
                let lower_name = name.to_lowercase();
                if lower_name == "description" {
                    if let Some(content) = el.get_attribute("content") {
                        *desc_ref.borrow_mut() =
                            Some(clean_whitespace(&decode_html_entities(&content)));
                    }
                } else if lower_name == "robots" {
                    if let Some(content) = el.get_attribute("content") {
                        let flags = parse_robots_directives(&content);
                        robots_ref.borrow_mut().insert(flags);
                    }
                } else if lower_name == "viewport" {
                    if let Some(content) = el.get_attribute("content") {
                        *viewport_ref.borrow_mut() =
                            Some(CompactString::new(clean_whitespace(&content)));
                    }
                } else if lower_name.starts_with("twitter:") {
                    if let Some(content) = el.get_attribute("content") {
                        tw_ref.borrow_mut().push((
                            CompactString::new(&lower_name),
                            clean_whitespace(&decode_html_entities(&content)),
                        ));
                    }
                }
            }

            if let Some(prop) = el.get_attribute("property") {
                let lower_prop = prop.to_lowercase();
                if lower_prop.starts_with("og:") {
                    if let Some(content) = el.get_attribute("content") {
                        og_ref.borrow_mut().push((
                            CompactString::new(&lower_prop),
                            clean_whitespace(&decode_html_entities(&content)),
                        ));
                    }
                }
            }

            if let Some(cs) = el.get_attribute("charset") {
                *charset_ref.borrow_mut() = Some(CompactString::new(cs.trim().to_lowercase()));
            } else if let Some(http_equiv) = el.get_attribute("http-equiv") {
                if http_equiv.eq_ignore_ascii_case("content-type") {
                    if let Some(content) = el.get_attribute("content") {
                        if let Some(idx) = content.to_lowercase().find("charset=") {
                            let cs = &content[idx + 8..].trim();
                            *charset_ref.borrow_mut() = Some(CompactString::new(cs.to_lowercase()));
                        }
                    }
                }
            }

            Ok(())
        }));
    }

    // 4. Link tags (Canonical & Hreflang)
    {
        let canon_ref = Rc::clone(&canonical);
        let hreflang_ref = Rc::clone(&hreflangs);
        let base = base_url.to_string();

        settings = settings.append_element_content_handler(element!("link", move |el| {
            if let Some(rel) = el.get_attribute("rel") {
                let lower_rel = rel.to_lowercase();
                if lower_rel.contains("canonical") {
                    if let Some(href) = el.get_attribute("href") {
                        if let Ok(resolved) = resolve_relative(&base, &href) {
                            *canon_ref.borrow_mut() = Some(resolved);
                        }
                    }
                } else if lower_rel.contains("alternate") {
                    if let Some(lang) = el.get_attribute("hreflang") {
                        if let Some(href) = el.get_attribute("href") {
                            if let Ok(resolved) = resolve_relative(&base, &href) {
                                hreflang_ref.borrow_mut().push(HreflangTag {
                                    lang_code: CompactString::new(lang.trim()),
                                    target_url: resolved,
                                    is_reciprocal: false,
                                });
                            }
                        }
                    }
                }
            }
            Ok(())
        }));
    }

    // 5. Headings (H1, H2, H3)
    {
        let h1s = Rc::clone(&h1_list);
        let cur_h1 = Rc::clone(&current_h1);
        settings = settings.append_element_content_handler(element!("h1", move |el| {
            if let Some(prev) = cur_h1.borrow_mut().take() {
                let cleaned = clean_whitespace(&decode_html_entities(&prev));
                if !cleaned.is_empty() {
                    h1s.borrow_mut().push(cleaned);
                }
            }
            *cur_h1.borrow_mut() = Some(String::new());

            let cur = Rc::clone(&cur_h1);
            let out = Rc::clone(&h1s);
            el.on_end_tag(end_tag!(move |_| {
                if let Some(h) = cur.borrow_mut().take() {
                    let cleaned = clean_whitespace(&decode_html_entities(&h));
                    if !cleaned.is_empty() {
                        out.borrow_mut().push(cleaned);
                    }
                }
                Ok(())
            }))?;
            Ok(())
        }));

        let h1_t = Rc::clone(&current_h1);
        settings = settings.append_element_content_handler(text!("h1", move |t| {
            if let Some(ref mut buf) = *h1_t.borrow_mut() {
                buf.push_str(t.as_str());
            }
            Ok(())
        }));
    }

    {
        let h2s = Rc::clone(&h2_list);
        let cur_h2 = Rc::clone(&current_h2);
        settings = settings.append_element_content_handler(element!("h2", move |el| {
            if let Some(prev) = cur_h2.borrow_mut().take() {
                let cleaned = clean_whitespace(&decode_html_entities(&prev));
                if !cleaned.is_empty() {
                    h2s.borrow_mut().push(cleaned);
                }
            }
            *cur_h2.borrow_mut() = Some(String::new());

            let cur = Rc::clone(&cur_h2);
            let out = Rc::clone(&h2s);
            el.on_end_tag(end_tag!(move |_| {
                if let Some(h) = cur.borrow_mut().take() {
                    let cleaned = clean_whitespace(&decode_html_entities(&h));
                    if !cleaned.is_empty() {
                        out.borrow_mut().push(cleaned);
                    }
                }
                Ok(())
            }))?;
            Ok(())
        }));

        let h2_t = Rc::clone(&current_h2);
        settings = settings.append_element_content_handler(text!("h2", move |t| {
            if let Some(ref mut buf) = *h2_t.borrow_mut() {
                buf.push_str(t.as_str());
            }
            Ok(())
        }));
    }

    {
        let h3s = Rc::clone(&h3_list);
        let cur_h3 = Rc::clone(&current_h3);
        settings = settings.append_element_content_handler(element!("h3", move |el| {
            if let Some(prev) = cur_h3.borrow_mut().take() {
                let cleaned = clean_whitespace(&decode_html_entities(&prev));
                if !cleaned.is_empty() {
                    h3s.borrow_mut().push(cleaned);
                }
            }
            *cur_h3.borrow_mut() = Some(String::new());

            let cur = Rc::clone(&cur_h3);
            let out = Rc::clone(&h3s);
            el.on_end_tag(end_tag!(move |_| {
                if let Some(h) = cur.borrow_mut().take() {
                    let cleaned = clean_whitespace(&decode_html_entities(&h));
                    if !cleaned.is_empty() {
                        out.borrow_mut().push(cleaned);
                    }
                }
                Ok(())
            }))?;
            Ok(())
        }));

        let h3_t = Rc::clone(&current_h3);
        settings = settings.append_element_content_handler(text!("h3", move |t| {
            if let Some(ref mut buf) = *h3_t.borrow_mut() {
                buf.push_str(t.as_str());
            }
            Ok(())
        }));
    }

    // 6. Hyperlinks (A tag)
    {
        let links_ref = Rc::clone(&links);
        let cur_link = Rc::clone(&current_link);
        let base = base_url.to_string();

        settings = settings.append_element_content_handler(element!("a", move |el| {
            if let Some(prev) = cur_link.borrow_mut().take() {
                flush_link(prev, &base, &mut links_ref.borrow_mut());
            }

            if let Some(href) = el.get_attribute("href") {
                let is_nofollow = el
                    .get_attribute("rel")
                    .map(|r| r.to_lowercase().contains("nofollow"))
                    .unwrap_or(false);

                *cur_link.borrow_mut() = Some(PendingLink {
                    href,
                    is_nofollow,
                    anchor_text: String::new(),
                    is_image_link: false,
                });

                let cur = Rc::clone(&cur_link);
                let out = Rc::clone(&links_ref);
                let b = base.clone();
                el.on_end_tag(end_tag!(move |_| {
                    if let Some(link) = cur.borrow_mut().take() {
                        flush_link(link, &b, &mut out.borrow_mut());
                    }
                    Ok(())
                }))?;
            }
            Ok(())
        }));

        let cur_text = Rc::clone(&current_link);
        settings = settings.append_element_content_handler(text!("a", move |t| {
            if let Some(ref mut link) = *cur_text.borrow_mut() {
                link.anchor_text.push_str(t.as_str());
            }
            Ok(())
        }));

        let cur_img = Rc::clone(&current_link);
        settings = settings.append_element_content_handler(element!("a img", move |_| {
            if let Some(ref mut link) = *cur_img.borrow_mut() {
                link.is_image_link = true;
            }
            Ok(())
        }));
    }

    // 7. Image Resources
    {
        let images_ref = Rc::clone(&images);
        let base = base_url.to_string();

        settings = settings.append_element_content_handler(element!("img", move |el| {
            if let Some(src) = el.get_attribute("src") {
                if let Ok(resolved_src) = resolve_relative(&base, &src) {
                    let alt = el
                        .get_attribute("alt")
                        .map(|a| clean_whitespace(&decode_html_entities(&a)));
                    let width = el
                        .get_attribute("width")
                        .and_then(|w| w.parse::<u32>().ok());
                    let height = el
                        .get_attribute("height")
                        .and_then(|h| h.parse::<u32>().ok());
                    let has_dimensions = width.is_some() && height.is_some();

                    images_ref.borrow_mut().push(ImageResource {
                        src_url: resolved_src,
                        alt_text: alt,
                        width,
                        height,
                        size_bytes: None,
                        has_dimensions,
                        is_broken: false,
                    });
                }
            }
            Ok(())
        }));
    }

    // 8. Structured Data (JSON-LD)
    {
        let buf_ref = Rc::clone(&json_ld_buf);
        let schemas_ref = Rc::clone(&schemas);

        settings = settings.append_element_content_handler(element!(
            "script[type=\"application/ld+json\"]",
            move |el| {
                let b = Rc::clone(&buf_ref);
                let out = Rc::clone(&schemas_ref);
                el.on_end_tag(end_tag!(move |_| {
                    let raw = b.borrow_mut().split_off(0);
                    let parsed = parse_json_ld(&raw);
                    out.borrow_mut().extend(parsed);
                    Ok(())
                }))?;
                Ok(())
            }
        ));

        let text_ref = Rc::clone(&json_ld_buf);
        settings = settings.append_element_content_handler(text!(
            "script[type=\"application/ld+json\"]",
            move |t| {
                text_ref.borrow_mut().push_str(t.as_str());
                Ok(())
            }
        ));
    }

    // 9. Editorial Content & Text Exclusion
    {
        let depth_ref = Rc::clone(&exclusion_depth);
        settings = settings.append_element_content_handler(element!(
            "head, nav, header, footer, script, style, noscript",
            move |el| {
                *depth_ref.borrow_mut() += 1;
                let d = Rc::clone(&depth_ref);
                el.on_end_tag(end_tag!(move |_| {
                    let mut borrowed = d.borrow_mut();
                    if *borrowed > 0 {
                        *borrowed -= 1;
                    }
                    Ok(())
                }))?;
                Ok(())
            }
        ));

        let depth_t = Rc::clone(&exclusion_depth);
        let editorial_t = Rc::clone(&editorial_text);
        settings = settings.append_element_content_handler(text!("*", move |t| {
            if *depth_t.borrow() == 0 {
                editorial_t.borrow_mut().push_str(t.as_str());
                editorial_t.borrow_mut().push(' ');
            }
            Ok(())
        }));
    }

    // Execute Streaming Rewriter
    let mut rewriter = HtmlRewriter::new(settings, |_chunk: &[u8]| {});
    rewriter
        .write(html.as_bytes())
        .map_err(|e| SeoError::Internal(format!("Streaming HTML parse error: {e}")))?;
    rewriter
        .end()
        .map_err(|e| SeoError::Internal(format!("Streaming HTML finish error: {e}")))?;

    // Flush any pending trailing items
    if let Some(pending) = current_link.borrow_mut().take() {
        flush_link(pending, base_url, &mut links.borrow_mut());
    }
    if let Some(h) = current_h1.borrow_mut().take() {
        let cleaned = clean_whitespace(&decode_html_entities(&h));
        if !cleaned.is_empty() {
            h1_list.borrow_mut().push(cleaned);
        }
    }
    if let Some(h) = current_h2.borrow_mut().take() {
        let cleaned = clean_whitespace(&decode_html_entities(&h));
        if !cleaned.is_empty() {
            h2_list.borrow_mut().push(cleaned);
        }
    }
    if let Some(h) = current_h3.borrow_mut().take() {
        let cleaned = clean_whitespace(&decode_html_entities(&h));
        if !cleaned.is_empty() {
            h3_list.borrow_mut().push(cleaned);
        }
    }

    // Process editorial content metrics
    let clean_body = clean_whitespace(&editorial_text.borrow());
    let word_count = count_words(&clean_body);
    let content_hash = compute_content_hash(&clean_body);
    let simhash = compute_simhash(&clean_body);

    let raw_title = title_buf.take();
    let title = if raw_title.trim().is_empty() {
        None
    } else {
        Some(clean_whitespace(&decode_html_entities(&raw_title)))
    };

    let all_h1 = h1_list.take();
    let h1_primary = all_h1.first().cloned();
    let h1_count = all_h1.len() as u16;

    let final_meta_desc = meta_desc.take();
    let final_canonical = canonical.take();
    let final_html_lang = html_lang.take();
    let final_charset = charset.take();
    let final_viewport = viewport.take();
    let final_robots = *robots_flags.borrow();
    let final_h2 = h2_list.take();
    let final_h3 = h3_list.take();
    let final_links = links.take();
    let final_images = images.take();
    let final_schemas = schemas.take();
    let final_hreflangs = hreflangs.take();
    let final_og = open_graph.take();
    let final_tw = twitter_cards.take();

    Ok(ParsedPage {
        title,
        meta_description: final_meta_desc,
        canonical_url: final_canonical,
        html_lang: final_html_lang,
        charset: final_charset,
        viewport: final_viewport,
        robots_flags: final_robots,
        h1_primary,
        h1_count,
        h2_headings: final_h2,
        h3_headings: final_h3,
        word_count,
        content_hash,
        simhash,
        links: final_links,
        images: final_images,
        schemas: final_schemas,
        hreflangs: final_hreflangs,
        open_graph: final_og,
        twitter_cards: final_tw,
    })
}

/// Validates, normalizes, and appends a pending hyperlink into the discovered links collection.
///
/// Skips empty hrefs, in-page fragments (`#...`), and non-HTTP protocols (`javascript:`, `mailto:`, `tel:`).
/// Resolves relative paths against the base URL, decodes anchor entities, and computes the 64-bit target URL hash.
fn flush_link(pending: PendingLink, base_url: &str, links: &mut Vec<DiscoveredLink>) {
    let trimmed = pending.href.trim();
    if trimmed.is_empty()
        || trimmed.starts_with('#')
        || trimmed.starts_with("javascript:")
        || trimmed.starts_with("mailto:")
        || trimmed.starts_with("tel:")
    {
        return;
    }

    if let Ok(resolved) = resolve_relative(base_url, trimmed) {
        let hash = url_hash(&resolved);
        let internal = is_internal(&resolved, base_url);
        links.push(DiscoveredLink {
            source_url: base_url.to_string(),
            target_url: resolved,
            target_url_hash: hash,
            anchor_text: clean_whitespace(&decode_html_entities(&pending.anchor_text)),
            is_internal: internal,
            is_nofollow: pending.is_nofollow,
            is_image_link: pending.is_image_link,
            status_code: None,
        });
    }
}
