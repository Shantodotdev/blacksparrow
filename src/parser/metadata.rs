//! # HTML Metadata Extraction
//!
//! Extraction and normalization helpers for meta robots indexing directives,
//! HTML entity decoding, and whitespace sanitization.
//!
//! ## Meta Robots Directives (RFC 9309 & Search Engine Standards)
//!
//! Webmasters communicate crawl and indexation permissions via `<meta name="robots" content="...">`.
//! These directives are comma-separated and case-insensitive:
//!
//! - `noindex`: Instructs search engines not to show this URL in search results.
//! - `nofollow`: Instructs search engines not to crawl hyperlinks discovered on this page.
//! - `none`: Shorthand equivalent to specifying both `noindex, nofollow`.
//! - `nosnippet`: Prevents search results from displaying text snippets or video previews.
//! - `noimageindex`: Prevents search engines from indexing images hosted on this page.
//! - `noarchive`: Prevents search engines from offering cached versions of this page in SERPs.
//!
//! In SEO Lens, these flags are mapped directly into a high-performance 1-byte [`RobotsFlags`] bitfield.
//!
//! ## Entity Decoding & Whitespace Sanitization
//!
//! Title tags and meta descriptions frequently contain HTML entities (e.g. `&amp;`, `&#39;`, `&quot;`)
//! or awkward multiline whitespace from templating engines. The sanitization helpers clean and collapse
//! these strings into canonical text representations before saving them to storage or evaluating
//! length boundaries.

use crate::core::models::RobotsFlags;

/// Parses a meta robots content string into a memory-efficient [`RobotsFlags`] bitfield.
///
/// Directives are parsed case-insensitively. The `none` directive automatically sets
/// both `NOINDEX` and `NOFOLLOW` bits.
///
/// # Examples
///
/// ```rust
/// use seo_lens::parser::metadata::parse_robots_directives;
/// use seo_lens::core::models::RobotsFlags;
///
/// let flags = parse_robots_directives("noindex, nofollow, nosnippet");
/// assert!(flags.contains(RobotsFlags::NOINDEX));
/// assert!(flags.contains(RobotsFlags::NOFOLLOW));
/// assert!(flags.contains(RobotsFlags::NOSNIPPET));
/// assert!(!flags.contains(RobotsFlags::NOARCHIVE));
///
/// // The "none" shortcut expands to both NOINDEX and NOFOLLOW
/// let none_flags = parse_robots_directives("NONE");
/// assert!(none_flags.contains(RobotsFlags::NOINDEX | RobotsFlags::NOFOLLOW));
/// ```
pub fn parse_robots_directives(content: &str) -> RobotsFlags {
    let mut flags = RobotsFlags::NONE;

    for directive in content.split(',') {
        let trimmed = directive.trim().to_lowercase();
        match trimmed.as_str() {
            "none" => {
                flags.insert(RobotsFlags::NOINDEX);
                flags.insert(RobotsFlags::NOFOLLOW);
            }
            "noindex" => flags.insert(RobotsFlags::NOINDEX),
            "nofollow" => flags.insert(RobotsFlags::NOFOLLOW),
            "nosnippet" => flags.insert(RobotsFlags::NOSNIPPET),
            "noimageindex" => flags.insert(RobotsFlags::NOIMAGEINDEX),
            "noarchive" => flags.insert(RobotsFlags::NOARCHIVE),
            _ => {}
        }
    }

    flags
}

/// Decodes standard HTML entities commonly found in titles and meta tags.
///
/// Handles named entities (`&amp;`, `&lt;`, `&gt;`, `&quot;`, `&apos;`, `&nbsp;`)
/// and numeric entities (`&#39;`).
///
/// # Examples
///
/// ```rust
/// use seo_lens::parser::metadata::decode_html_entities;
///
/// let encoded = "Tips &amp; Tricks: &quot;SEO 101&#39;s Guide&quot; &lt;v2&gt;";
/// let decoded = decode_html_entities(encoded);
/// assert_eq!(decoded, "Tips & Tricks: \"SEO 101's Guide\" <v2>");
/// ```
pub fn decode_html_entities(input: &str) -> String {
    input
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
}

/// Sanitizes extracted text by collapsing duplicate whitespace and trimming.
///
/// Replaces consecutive whitespace characters (spaces, tabs, newlines) with a single
/// ASCII space and removes leading and trailing whitespace.
///
/// # Examples
///
/// ```rust
/// use seo_lens::parser::metadata::clean_whitespace;
///
/// let raw = "   \n\t Hello \t  world!\n \n";
/// assert_eq!(clean_whitespace(raw), "Hello world!");
/// ```
pub fn clean_whitespace(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut in_whitespace = false;

    for c in input.chars() {
        if c.is_whitespace() {
            if !in_whitespace {
                result.push(' ');
                in_whitespace = true;
            }
        } else {
            result.push(c);
            in_whitespace = false;
        }
    }

    result.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_robots_directives() {
        let flags = parse_robots_directives("noindex, nofollow, nosnippet");
        assert!(flags.contains(RobotsFlags::NOINDEX));
        assert!(flags.contains(RobotsFlags::NOFOLLOW));
        assert!(flags.contains(RobotsFlags::NOSNIPPET));
        assert!(!flags.contains(RobotsFlags::NOARCHIVE));

        let none_flags = parse_robots_directives("none");
        assert!(none_flags.contains(RobotsFlags::NOINDEX));
        assert!(none_flags.contains(RobotsFlags::NOFOLLOW));
    }

    #[test]
    fn test_decode_html_entities_and_whitespace() {
        let raw = "  Foo &amp; Bar &lt;Baz&gt; &quot;Quux&#39;s&quot;   ";
        let decoded = decode_html_entities(raw);
        let cleaned = clean_whitespace(&decoded);
        assert_eq!(cleaned, "Foo & Bar <Baz> \"Quux's\"");
    }
}
