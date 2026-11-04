//! # Page Brand & Content Analysis
//!
//! Reads one HTML page (untrusted, never executed) into what site extraction needs:
//!
//! - the page as Markdown, links and `tel:`/`mailto:` targets kept, for downstream text models;
//! - its links, metadata (title, description, language, theme colour, site name);
//! - the stylesheets to fetch, inline `<style>` blocks and Google Fonts links;
//! - logo candidates, best first: a logo image or inline SVG in the header or navigation, a logo
//!   elsewhere, the Apple touch icon, the `og:image`, then favicons;
//! - schema.org JSON-LD facts about the business (name, phones, emails, addresses, profiles);
//! - signals that the page is an empty JavaScript application shell that needs rendering.
//!
//! The document is parsed once into a DOM with `html5ever` (via `scraper`), read, and dropped,
//! so memory stays bounded by the largest single page.

use super::css::{google_fonts, parse_color, ClassRoles};
use scraper::node::Node;
use scraper::{ElementRef, Html};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use url::Url;

/// Largest inline SVG kept as a logo; anything bigger is an illustration.
pub const MAX_INLINE_SVG_CHARS: usize = 200_000;
/// Longest Markdown produced for one page.
pub const MAX_MARKDOWN_CHARS: usize = 300_000;
const MAX_STYLESHEETS_LISTED: usize = 40;
const MAX_INLINE_STYLE_CHARS: usize = 2 * 1024 * 1024;
const MAX_LOGO_CANDIDATES: usize = 12;
const MAX_LINKS: usize = 400;
const MAX_LINK_TEXT: usize = 200;
const MAX_DEPTH: usize = 256;
/// Deepest element nesting parsed; real pages stay far below it.
const MAX_PARSE_NESTING: usize = 1000;
const MAX_JSON_LD_BLOCKS: usize = 20;
const MAX_JSON_LD_CHARS: usize = 200_000;

/// Why an image is a logo candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LogoReason {
    /// A logo-named image or SVG inside `<header>` or `<nav>`.
    HeaderLogo,
    /// The `logo` of a schema.org Organization.
    SchemaLogo,
    /// A logo-named image or SVG elsewhere.
    LogoHint,
    /// `<link rel="apple-touch-icon">`.
    AppleTouchIcon,
    /// `<meta property="og:image">`.
    OgImage,
    /// A favicon `<link rel="icon">`.
    Icon,
}

/// A place the brand's logo may be.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LogoCandidate {
    /// Why this is a candidate; lower ranks first.
    pub reason: LogoReason,
    /// Absolute http(s) image URL; `None` for an inline SVG.
    pub url: Option<String>,
    /// A standalone SVG document; `None` for an image URL.
    pub svg: Option<String>,
}

/// A hyperlink on the page.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PageLink {
    /// Visible text (or label) of the link, whitespace collapsed.
    pub text: String,
    /// Absolute http(s) target.
    pub url: String,
}

/// Facts a page states about its business in schema.org JSON-LD.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct StructuredBusiness {
    /// `name` values of Organization/LocalBusiness-like nodes.
    pub names: Vec<String>,
    /// `telephone` values.
    pub telephones: Vec<String>,
    /// `email` values.
    pub emails: Vec<String>,
    /// Postal addresses, one string each.
    pub addresses: Vec<String>,
    /// `sameAs` profile URLs.
    pub same_as: Vec<String>,
    /// `logo` URLs.
    pub logos: Vec<String>,
    /// `description` values.
    pub descriptions: Vec<String>,
}

/// Everything read from one page.
#[derive(Debug, Clone, Default)]
pub struct PageAnalysis {
    /// `<title>`.
    pub title: Option<String>,
    /// Meta or Open Graph description.
    pub description: Option<String>,
    /// `<html lang>`.
    pub lang: Option<String>,
    /// `theme-color` meta as `#RRGGBB`.
    pub theme_color: Option<String>,
    /// `og:site_name`.
    pub site_name: Option<String>,
    /// External stylesheets to fetch, in page order.
    pub stylesheet_urls: Vec<String>,
    /// `<style>` block contents.
    pub inline_styles: Vec<String>,
    /// Families from Google Fonts `<link>`s with their weights.
    pub google_fonts: BTreeMap<String, Vec<u16>>,
    /// http(s) links, in page order, unique by URL.
    pub links: Vec<PageLink>,
    /// Logo candidates, best first.
    pub logo_candidates: Vec<LogoCandidate>,
    /// The page as Markdown.
    pub markdown: String,
    /// Words of visible text.
    pub word_count: u32,
    /// `<script>` elements.
    pub script_count: u32,
    /// Whether an application mount point (`#root`, `#app`, `#__next`, ...) is nearly empty.
    pub empty_app_root: bool,
    /// Whether a `<noscript>` asks the visitor to enable JavaScript.
    pub noscript_requires_js: bool,
    /// schema.org business facts.
    pub structured: StructuredBusiness,
    /// Kinds of text each class name is applied to.
    pub class_roles: ClassRoles,
}

impl PageAnalysis {
    /// Whether the page looks like a client-rendered shell whose content needs JavaScript.
    pub fn looks_like_js_shell(&self) -> bool {
        (self.empty_app_root && self.word_count < 200)
            || (self.noscript_requires_js && self.word_count < 120)
            || (self.script_count > 0 && self.word_count < 40)
    }
}

/// The URL relative links resolve against: the page's first `<base href>` (http or https only)
/// resolved against the page URL, else the page URL itself.
fn document_base(document: &Html, page: &Url) -> Url {
    document
        .root_element()
        .descendent_elements()
        .find(|element| element.value().name() == "base" && element.value().attr("href").is_some())
        .and_then(|element| element.value().attr("href"))
        .and_then(|href| page.join(href.trim()).ok())
        .filter(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
        .unwrap_or_else(|| page.clone())
}

/// Analyses a page; relative URLs resolve against `base`, or against the page's `<base href>`.
pub fn analyze_page(html: &str, base: &Url) -> PageAnalysis {
    let document = Html::parse_document(limit_nesting(html, MAX_PARSE_NESTING));
    let effective_base = document_base(&document, base);
    let base = &effective_base;
    let mut analysis = PageAnalysis::default();
    let mut candidates: Vec<(usize, LogoCandidate)> = Vec::new();
    let root = document.root_element();
    analysis.lang = root
        .value()
        .attr("lang")
        .map(|lang| lang.trim().to_string())
        .filter(|lang| !lang.is_empty() && lang.len() <= 35);

    let mut style_chars = 0usize;
    let mut json_ld: Vec<String> = Vec::new();
    for element in root.descendent_elements() {
        let name = element.value().name();
        match name {
            "title" if analysis.title.is_none() => {
                let title = collapse(&element.text().collect::<String>());
                analysis.title = (!title.is_empty()).then(|| truncate(&title, 300));
            }
            "meta" => read_meta(element, base, &mut analysis, &mut candidates),
            "link" => read_link(element, base, &mut analysis, &mut candidates),
            "style" => {
                let css: String = element.text().collect();
                style_chars += css.len();
                if style_chars <= MAX_INLINE_STYLE_CHARS {
                    analysis.inline_styles.push(css);
                }
            }
            "script" => {
                analysis.script_count += 1;
                let kind = element.value().attr("type").unwrap_or("");
                if kind.eq_ignore_ascii_case("application/ld+json")
                    && json_ld.len() < MAX_JSON_LD_BLOCKS
                {
                    let text: String = element.text().collect();
                    if text.len() <= MAX_JSON_LD_CHARS {
                        json_ld.push(text);
                    }
                }
            }
            "noscript" => {
                let text = element.text().collect::<String>().to_lowercase();
                if text.contains("javascript") {
                    analysis.noscript_requires_js = true;
                }
            }
            "img" => read_image(element, base, &mut candidates),
            "svg" => read_svg(element, base, &mut candidates),
            "a" => read_anchor(element, base, &mut analysis.links),
            _ => {}
        }
        record_class_roles(element, &mut analysis.class_roles);
        if is_app_root(element) && element.text().map(str::len).sum::<usize>() < 50 {
            analysis.empty_app_root = true;
        }
    }

    for block in &json_ld {
        if let Ok(value) = serde_json::from_str::<Value>(block) {
            read_json_ld(&value, base, &mut analysis.structured, 0);
        }
    }

    candidates.sort_by_key(|(order, candidate)| (candidate.reason, *order));
    let mut seen: Vec<String> = Vec::new();
    for (_, candidate) in candidates {
        let key = candidate
            .url
            .clone()
            .or_else(|| candidate.svg.clone())
            .unwrap_or_default();
        if !seen.contains(&key) {
            seen.push(key);
            analysis.logo_candidates.push(candidate);
        }
        if analysis.logo_candidates.len() >= MAX_LOGO_CANDIDATES {
            break;
        }
    }

    let body = root
        .descendent_elements()
        .find(|element| element.value().name() == "body")
        .unwrap_or(root);
    let mut writer = MarkdownWriter::new(base);
    writer.walk(body, 0);
    analysis.word_count = writer.words;
    analysis.markdown = writer.finish();
    analysis
}

/// `html` cut at the first tag that nests deeper than `max_depth` elements. The HTML parser's
/// time grows with the square of the nesting depth, so a hostile page of tens of thousands of
/// unclosed `<div>`s would otherwise keep a CPU busy for minutes. Depth is counted from tags
/// alone: void elements, elements the parser closes on its own (`p`, `li`, table cells) and the
/// raw text of `script`/`style` are not counted.
fn limit_nesting(html: &str, max_depth: usize) -> &str {
    const UNCOUNTED: &[&str] = &[
        "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param",
        "source", "track", "wbr", "p", "li", "dt", "dd", "tr", "td", "th", "option", "thead",
        "tbody", "tfoot", "html", "head", "body",
    ];
    let bytes = html.as_bytes();
    let mut depth = 0usize;
    let mut index = 0usize;
    while let Some(offset) = bytes[index..].iter().position(|b| *b == b'<') {
        let start = index + offset;
        let closing = bytes.get(start + 1) == Some(&b'/');
        let name_start = start + if closing { 2 } else { 1 };
        let name_end = bytes[name_start.min(bytes.len())..]
            .iter()
            .position(|b| !(b.is_ascii_alphanumeric() || *b == b'-' || *b == b':'))
            .map_or(bytes.len(), |n| name_start + n);
        index = name_end.max(start + 1);
        if name_end == name_start {
            continue;
        }
        let name = html[name_start..name_end].to_ascii_lowercase();
        let tag_end = bytes[name_end..]
            .iter()
            .position(|b| *b == b'>')
            .map_or(bytes.len(), |n| name_end + n);
        if closing {
            if !UNCOUNTED.contains(&name.as_str()) {
                depth = depth.saturating_sub(1);
            }
            index = tag_end.max(index);
            continue;
        }
        let self_closing = tag_end > 0 && bytes.get(tag_end - 1) == Some(&b'/');
        if matches!(name.as_str(), "script" | "style" | "textarea" | "title") && !self_closing {
            // Raw text: skip to the matching end tag.
            let needle = format!("</{name}").into_bytes();
            index = bytes[tag_end..]
                .windows(needle.len())
                .position(|window| window.eq_ignore_ascii_case(&needle))
                .map_or(bytes.len(), |n| tag_end + n);
            continue;
        }
        if !UNCOUNTED.contains(&name.as_str()) && !self_closing {
            depth += 1;
            if depth > max_depth {
                return &html[..start];
            }
        }
        index = tag_end.max(index);
    }
    html
}

fn read_meta(
    element: ElementRef<'_>,
    base: &Url,
    analysis: &mut PageAnalysis,
    candidates: &mut Vec<(usize, LogoCandidate)>,
) {
    let value = element.value();
    let name = value
        .attr("name")
        .or_else(|| value.attr("property"))
        .unwrap_or("")
        .to_ascii_lowercase();
    let content = value.attr("content").unwrap_or("");
    match name.as_str() {
        "theme-color" => {
            if let Some(color) = parse_color(content) {
                analysis.theme_color = Some(color);
            }
        }
        "description" | "og:description" if analysis.description.is_none() => {
            let text = collapse(content);
            analysis.description = (!text.is_empty()).then(|| truncate(&text, 2000));
        }
        "og:site_name" if analysis.site_name.is_none() => {
            let text = collapse(content);
            analysis.site_name = (!text.is_empty()).then(|| truncate(&text, 200));
        }
        "og:image" => {
            if let Some(url) = web_url(base, content) {
                candidates.push((
                    candidates.len(),
                    LogoCandidate {
                        reason: LogoReason::OgImage,
                        url: Some(url),
                        svg: None,
                    },
                ));
            }
        }
        _ => {}
    }
}

fn read_link(
    element: ElementRef<'_>,
    base: &Url,
    analysis: &mut PageAnalysis,
    candidates: &mut Vec<(usize, LogoCandidate)>,
) {
    let value = element.value();
    let rels: Vec<String> = value
        .attr("rel")
        .unwrap_or("")
        .to_ascii_lowercase()
        .split_whitespace()
        .map(str::to_string)
        .collect();
    let Some(url) = value.attr("href").and_then(|href| web_url(base, href)) else {
        return;
    };
    let has = |rel: &str| rels.iter().any(|r| r == rel);
    if has("stylesheet") {
        let families = google_fonts(&url);
        if !families.is_empty() {
            for (family, weights) in families {
                let entry = analysis.google_fonts.entry(family).or_default();
                for weight in weights {
                    if !entry.contains(&weight) {
                        entry.push(weight);
                    }
                }
                entry.sort_unstable();
            }
        } else if analysis.stylesheet_urls.len() < MAX_STYLESHEETS_LISTED
            && !analysis.stylesheet_urls.contains(&url)
        {
            analysis.stylesheet_urls.push(url);
        }
    } else if has("apple-touch-icon") || has("apple-touch-icon-precomposed") {
        candidates.push((
            candidates.len(),
            LogoCandidate {
                reason: LogoReason::AppleTouchIcon,
                url: Some(url),
                svg: None,
            },
        ));
    } else if has("icon") || has("shortcut") || has("mask-icon") {
        candidates.push((
            candidates.len(),
            LogoCandidate {
                reason: LogoReason::Icon,
                url: Some(url),
                svg: None,
            },
        ));
    }
}

/// Most class names recorded per page for class-role matching.
const MAX_ROLE_CLASSES: usize = 2000;

fn record_class_roles(element: ElementRef<'_>, roles: &mut ClassRoles) {
    let value = element.value();
    let name = value.name();
    let mut role = match name {
        "html" | "body" | "main" | "p" => Some("body"),
        "h1" | "h2" | "h3" => Some("headings"),
        "button" => Some("buttons"),
        "a" => Some("links"),
        _ => None,
    };
    if value
        .classes()
        .any(|class| class == "btn" || class == "button" || class.starts_with("btn-"))
    {
        role = Some("buttons");
    }
    let Some(role) = role else {
        return;
    };
    for class in value.classes() {
        if roles.len() >= MAX_ROLE_CLASSES && !roles.contains_key(class) {
            return;
        }
        let entry = roles.entry(class.to_string()).or_default();
        if !entry.contains(&role) {
            entry.push(role);
        }
    }
}

/// The address hidden by Cloudflare's email obfuscation: hex bytes whose first byte is the XOR
/// key of the rest.
pub fn decode_cloudflare_email(hex: &str) -> Option<String> {
    let bytes: Vec<u8> = (0..hex.len())
        .step_by(2)
        .map(|i| {
            hex.get(i..i + 2)
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
        })
        .collect::<Option<Vec<u8>>>()?;
    let (key, rest) = bytes.split_first()?;
    let decoded: String = rest.iter().map(|byte| char::from(byte ^ key)).collect();
    (decoded.contains('@') && decoded.is_ascii() && decoded.len() <= 254).then_some(decoded)
}

fn cloudflare_email(element: ElementRef<'_>) -> Option<String> {
    let value = element.value();
    if let Some(hex) = value.attr("data-cfemail") {
        return decode_cloudflare_email(hex);
    }
    let href = value.attr("href")?;
    let (_, hex) = href.split_once("/cdn-cgi/l/email-protection#")?;
    decode_cloudflare_email(hex)
}

/// Words that put a "logo" among many: partner and client carousels, not the business's own.
const NOT_OWN_LOGO_WORDS: &[&str] = &[
    "partner",
    "partners",
    "client",
    "clients",
    "sponsor",
    "sponsors",
    "customer",
    "customers",
    "carousel",
    "slider",
    "slick",
    "swiper",
    "marquee",
    "brands",
    "affiliate",
    "affiliates",
    "trusted",
    "featured",
];

/// Lowercase alphanumeric words of `value`.
fn words(value: &str) -> Vec<String> {
    value
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Whether `value` has the whole word `logo` and no word that makes it one of many logos.
fn names_own_logo(value: &str) -> bool {
    let words = words(value);
    words.iter().any(|word| word == "logo")
        && !words
            .iter()
            .any(|word| NOT_OWN_LOGO_WORDS.contains(&word.as_str()))
}

/// Whether the file name of an image URL says "logo" and its folders do not name a partner
/// list.
fn path_names_own_logo(src: &str) -> bool {
    let path = src.split(['?', '#']).next().unwrap_or("");
    let mut segments = path.rsplit('/');
    let file = segments.next().unwrap_or("");
    let folders = segments.collect::<Vec<_>>().join(" ");
    let folder_words = words(&folders);
    words(file).iter().any(|word| word == "logo")
        && !words(file)
            .iter()
            .any(|word| NOT_OWN_LOGO_WORDS.contains(&word.as_str()))
        && !folder_words
            .iter()
            .any(|word| NOT_OWN_LOGO_WORDS.contains(&word.as_str()))
}

/// Whether the element itself says "logo" in a naming attribute, as a whole word.
fn own_logo_hint(element: ElementRef<'_>) -> bool {
    let value = element.value();
    ["class", "id", "alt", "aria-label", "title"]
        .iter()
        .any(|name| value.attr(name).is_some_and(names_own_logo))
        || ["src", "data-src"]
            .iter()
            .any(|name| value.attr(name).is_some_and(path_names_own_logo))
}

/// Whether the element or one of its three nearest ancestors names itself a logo (a logo is
/// often an image inside a link or wrapper whose class says "logo").
fn logo_hinted(element: ElementRef<'_>) -> bool {
    own_logo_hint(element)
        || element
            .ancestors()
            .filter_map(ElementRef::wrap)
            .take(3)
            .any(own_logo_hint)
}

fn in_header(element: ElementRef<'_>) -> bool {
    element
        .ancestors()
        .filter_map(ElementRef::wrap)
        .any(|ancestor| matches!(ancestor.value().name(), "header" | "nav"))
}

/// Whether the element sits inside a link to the site's home page.
fn in_home_link(element: ElementRef<'_>, base: &Url) -> bool {
    element
        .ancestors()
        .filter_map(ElementRef::wrap)
        .filter(|ancestor| ancestor.value().name() == "a")
        .filter_map(|anchor| anchor.value().attr("href"))
        .filter_map(|href| base.join(href.trim()).ok())
        .any(|url| url.path() == "/" && url.query().is_none() && url.host_str() == base.host_str())
}

fn inside_svg(element: ElementRef<'_>) -> bool {
    element
        .ancestors()
        .filter_map(ElementRef::wrap)
        .any(|ancestor| ancestor.value().name() == "svg")
}

fn logo_reason(element: ElementRef<'_>, base: &Url) -> LogoReason {
    if in_header(element) || in_home_link(element, base) {
        LogoReason::HeaderLogo
    } else {
        LogoReason::LogoHint
    }
}

/// Sort key within a reason: logos in a link to the home page come before other logos.
fn logo_order(element: ElementRef<'_>, base: &Url, position: usize) -> usize {
    if in_home_link(element, base) {
        position
    } else {
        position + HOME_LINK_ORDER_GAP
    }
}

/// Added to the position of logos that do not link home, so they sort after those that do.
const HOME_LINK_ORDER_GAP: usize = 1_000_000;

fn read_image(element: ElementRef<'_>, base: &Url, candidates: &mut Vec<(usize, LogoCandidate)>) {
    let value = element.value();
    let src = value
        .attr("src")
        .filter(|src| !src.trim().is_empty() && !src.trim_start().starts_with("data:"))
        .or_else(|| value.attr("data-src"));
    let Some(url) = src.and_then(|src| web_url(base, src)) else {
        return;
    };
    if logo_hinted(element) {
        candidates.push((
            logo_order(element, base, candidates.len()),
            LogoCandidate {
                reason: logo_reason(element, base),
                url: Some(url),
                svg: None,
            },
        ));
    }
}

fn read_svg(element: ElementRef<'_>, base: &Url, candidates: &mut Vec<(usize, LogoCandidate)>) {
    if inside_svg(element) || !logo_hinted(element) {
        return;
    }
    let Some(markup) = sanitized_svg(element) else {
        return;
    };
    candidates.push((
        logo_order(element, base, candidates.len()),
        LogoCandidate {
            reason: logo_reason(element, base),
            url: None,
            svg: Some(markup),
        },
    ));
}

/// Elements an inline SVG logo may keep: shapes, text and paint servers.
const SVG_ELEMENTS: &[&str] = &[
    "svg",
    "g",
    "path",
    "circle",
    "ellipse",
    "rect",
    "line",
    "polyline",
    "polygon",
    "text",
    "tspan",
    "defs",
    "lineargradient",
    "radialgradient",
    "stop",
    "clippath",
    "mask",
    "use",
    "symbol",
    "title",
    "desc",
    "style",
];

/// Attributes an inline SVG logo may keep (compared in lowercase).
const SVG_ATTRIBUTES: &[&str] = &[
    "id",
    "class",
    "style",
    "d",
    "points",
    "x",
    "y",
    "x1",
    "x2",
    "y1",
    "y2",
    "cx",
    "cy",
    "r",
    "rx",
    "ry",
    "fx",
    "fy",
    "fr",
    "width",
    "height",
    "viewbox",
    "preserveaspectratio",
    "fill",
    "fill-opacity",
    "fill-rule",
    "stroke",
    "stroke-width",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-miterlimit",
    "stroke-dasharray",
    "stroke-dashoffset",
    "stroke-opacity",
    "opacity",
    "transform",
    "gradienttransform",
    "gradientunits",
    "spreadmethod",
    "offset",
    "stop-color",
    "stop-opacity",
    "clip-path",
    "clip-rule",
    "clippathunits",
    "mask",
    "maskunits",
    "maskcontentunits",
    "font-family",
    "font-size",
    "font-weight",
    "font-style",
    "letter-spacing",
    "text-anchor",
    "dominant-baseline",
    "dx",
    "dy",
    "rotate",
    "textlength",
    "lengthadjust",
    "display",
    "visibility",
    "color",
    "vector-effect",
    "href",
];

/// Most elements kept in a sanitised SVG.
const MAX_SVG_ELEMENTS: usize = 5000;
/// Deepest element nesting kept in a sanitised SVG.
const MAX_SVG_DEPTH: usize = 64;

/// A standalone SVG document rebuilt from the allow-listed elements and attributes of an inline
/// `<svg>`, or `None` when it is too large. Scripts, event handlers, `<foreignObject>`, external
/// references and everything else not on the lists are dropped, and every value is escaped.
fn sanitized_svg(root: ElementRef<'_>) -> Option<String> {
    let mut out = String::from(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\"",
    );
    let mut budget = MAX_SVG_ELEMENTS;
    write_svg_element(root, true, 0, &mut budget, &mut out)?;
    Some(out)
}

/// Writes `element` (already known to be an allow-listed element) and its allowed content. The
/// root's opening tag is already partly written, so it only gets its attributes appended.
fn write_svg_element(
    element: ElementRef<'_>,
    is_root: bool,
    depth: usize,
    budget: &mut usize,
    out: &mut String,
) -> Option<()> {
    if out.len() > MAX_INLINE_SVG_CHARS || depth > MAX_SVG_DEPTH || *budget == 0 {
        return None;
    }
    *budget -= 1;
    let name = element.value().name();
    if !is_root {
        out.push('<');
        out.push_str(name);
    }
    for (attribute, value) in element.value().attrs() {
        if let Some(safe) = safe_svg_attribute(attribute, value) {
            if is_root && attribute.eq_ignore_ascii_case("xmlns") {
                continue;
            }
            out.push(' ');
            out.push_str(attribute);
            out.push_str("=\"");
            out.push_str(&escape_xml(&safe));
            out.push('"');
        }
    }
    out.push('>');
    let lower = name.to_ascii_lowercase();
    let text_holder = matches!(
        lower.as_str(),
        "text" | "tspan" | "title" | "desc" | "style"
    );
    for child in element.children() {
        match child.value() {
            Node::Text(text) if text_holder => {
                if lower == "style" && !safe_svg_style_text(text) {
                    continue;
                }
                out.push_str(&escape_xml(text));
            }
            Node::Element(_) => {
                if let Some(child) = ElementRef::wrap(child) {
                    let child_name = child.value().name().to_ascii_lowercase();
                    if SVG_ELEMENTS.contains(&child_name.as_str()) && child_name != "svg" {
                        write_svg_element(child, false, depth + 1, budget, out)?;
                    }
                }
            }
            _ => {}
        }
    }
    out.push_str("</");
    out.push_str(name);
    out.push('>');
    Some(())
}

/// The value of an allow-listed SVG attribute if it is safe to keep: no escapes or comments, no
/// script URLs, and references (`href`, `url(...)`) only to ids inside the document.
fn safe_svg_attribute(name: &str, value: &str) -> Option<String> {
    let lower = name.to_ascii_lowercase();
    if !SVG_ATTRIBUTES.contains(&lower.as_str()) || value.len() > 20_000 {
        return None;
    }
    if lower == "href" {
        let trimmed = value.trim();
        return (trimmed.starts_with('#') && !trimmed.contains(char::is_whitespace))
            .then(|| trimmed.to_string());
    }
    safe_svg_css(value).then(|| value.to_string())
}

/// Whether a presentation value or style text is free of anything that loads or runs content.
fn safe_svg_css(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    if value.contains(['\\', '<']) || lower.contains("/*") || lower.contains('@') {
        return false;
    }
    if [
        "javascript:",
        "expression",
        "behavior",
        "binding",
        "image",
        "src(",
        "element(",
        "data:",
    ]
    .iter()
    .any(|bad| lower.contains(bad))
    {
        return false;
    }
    let mut rest = lower.as_str();
    while let Some(at) = rest.find("url(") {
        let target = rest[at + 4..].trim_start().trim_start_matches(['"', '\'']);
        if !target.starts_with('#') {
            return false;
        }
        rest = &rest[at + 4..];
    }
    true
}

/// Whether the text of a `<style>` element is safe to keep.
fn safe_svg_style_text(text: &str) -> bool {
    text.len() <= 20_000 && safe_svg_css(text)
}

fn escape_xml(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c if c.is_control() && !matches!(c, '\n' | '\t' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

fn read_anchor(element: ElementRef<'_>, base: &Url, links: &mut Vec<PageLink>) {
    if links.len() >= MAX_LINKS || is_hidden(element) {
        return;
    }
    let Some(url) = element
        .value()
        .attr("href")
        .and_then(|href| web_url(base, href))
    else {
        return;
    };
    if links.iter().any(|link| link.url == url) {
        return;
    }
    links.push(PageLink {
        text: truncate(&link_label(element), MAX_LINK_TEXT),
        url,
    });
}

/// The text of a link, or its label, image alt or title when it has no text.
fn link_label(element: ElementRef<'_>) -> String {
    let text = collapse(&element.text().collect::<String>());
    if !text.is_empty() {
        return text;
    }
    for name in ["aria-label", "title"] {
        if let Some(label) = element.value().attr(name) {
            let label = collapse(label);
            if !label.is_empty() {
                return label;
            }
        }
    }
    element
        .descendent_elements()
        .find_map(|child| child.value().attr("alt").map(collapse))
        .unwrap_or_default()
}

/// Whether an element is hidden from visitors for good. React's streaming server rendering
/// sends finished content in `<div hidden id="S:n">` blocks that a script moves into place, so
/// those count as visible; so do the `aria-hidden` icons inside visible links.
fn is_hidden(element: ElementRef<'_>) -> bool {
    let value = element.value();
    if value.attr("hidden").is_some() {
        return !value.id().map(|id| id.starts_with("S:")).unwrap_or(false);
    }
    let style = value
        .attr("style")
        .unwrap_or("")
        .to_ascii_lowercase()
        .replace(' ', "");
    style.contains("display:none") || style.contains("visibility:hidden")
}

fn is_app_root(element: ElementRef<'_>) -> bool {
    let value = element.value();
    matches!(
        value.id(),
        Some("root" | "app" | "__next" | "__nuxt" | "___gatsby" | "svelte" | "q-app")
    ) || value.name() == "app-root"
        || value.attr("ng-version").is_some()
        || value.attr("data-reactroot").is_some()
}

/// `value` resolved against `base` when it is an http(s) URL; `None` otherwise.
pub fn web_url(base: &Url, value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.len() > 4096 {
        return None;
    }
    let url = base.join(trimmed).ok()?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return None;
    }
    Some(url.to_string())
}

/// Whitespace runs collapsed to single spaces, trimmed.
pub fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// At most `max` characters of `text`.
pub fn truncate(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

const BUSINESS_TYPES: &[&str] = &[
    "organization",
    "corporation",
    "localbusiness",
    "store",
    "onlinestore",
    "onlinebusiness",
    "ngo",
    "restaurant",
    "bakery",
    "hotel",
    "motel",
    "resort",
    "hostel",
    "dentist",
    "physician",
    "hospital",
    "pharmacy",
    "medicalclinic",
    "medicalbusiness",
    "diagnosticlab",
    "professionalservice",
    "legalservice",
    "attorney",
    "accountingservice",
    "financialservice",
    "insuranceagency",
    "realestateagent",
    "travelagency",
    "beautysalon",
    "hairsalon",
    "nailsalon",
    "healthandbeautybusiness",
    "healthclub",
    "gym",
    "foodestablishment",
    "cafeorcoffeeshop",
    "barorpub",
    "fastfoodrestaurant",
    "icecreamshop",
    "winery",
    "brewery",
    "school",
    "educationalorganization",
    "preschool",
    "collegeoruniversity",
    "autodealer",
    "autorepair",
    "autobodyshop",
    "electrician",
    "plumber",
    "locksmith",
    "movingcompany",
    "homeandconstructionbusiness",
    "generalcontractor",
    "lodgingbusiness",
    "bedandbreakfast",
    "sportsactivitylocation",
    "entertainmentbusiness",
    "governmentoffice",
    "performinggroup",
];

/// The schema.org type name of `kind`: `https://schema.org/Dentist`, `schema:Dentist` and
/// `Dentist` all give `dentist`.
fn schema_type_name(kind: &str) -> String {
    let trimmed = kind.trim();
    let name = trimmed.rsplit(['/', ':', '#']).next().unwrap_or(trimmed);
    name.to_ascii_lowercase()
}

fn is_business_type(value: Option<&Value>) -> bool {
    let matches = |kind: &str| {
        let short = schema_type_name(kind);
        BUSINESS_TYPES.contains(&short.as_str())
            || short.ends_with("store")
            || short.ends_with("business")
            || short.ends_with("shop")
            || short.ends_with("clinic")
            || short.ends_with("salon")
            || short.ends_with("establishment")
    };
    match value {
        Some(Value::String(kind)) => matches(kind),
        Some(Value::Array(kinds)) => kinds
            .iter()
            .any(|k| k.as_str().map(matches).unwrap_or(false)),
        _ => false,
    }
}

fn strings(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::String(text)) => vec![collapse(text)],
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|item| item.as_str().map(collapse))
            .collect(),
        _ => Vec::new(),
    }
    .into_iter()
    .filter(|text| !text.is_empty() && text.len() <= 500)
    .collect()
}

fn push_all(target: &mut Vec<String>, values: Vec<String>) {
    for value in values {
        if target.len() < 20 && !target.contains(&value) {
            target.push(value);
        }
    }
}

fn postal_address(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(collapse(text)).filter(|t| !t.is_empty()),
        Value::Object(map) => {
            let mut parts: Vec<String> = Vec::new();
            for key in [
                "streetAddress",
                "addressLocality",
                "addressRegion",
                "postalCode",
                "addressCountry",
            ] {
                let part = match map.get(key) {
                    Some(Value::String(text)) => collapse(text),
                    Some(Value::Object(country)) => country
                        .get("name")
                        .and_then(Value::as_str)
                        .map(collapse)
                        .unwrap_or_default(),
                    _ => String::new(),
                };
                if !part.is_empty() && !parts.contains(&part) {
                    parts.push(part);
                }
            }
            (!parts.is_empty()).then(|| truncate(&parts.join(", "), 500))
        }
        _ => None,
    }
}

fn read_json_ld(value: &Value, base: &Url, found: &mut StructuredBusiness, depth: usize) {
    if depth > 8 {
        return;
    }
    match value {
        Value::Array(items) => {
            for item in items.iter().take(100) {
                read_json_ld(item, base, found, depth + 1);
            }
        }
        Value::Object(map) => {
            if is_business_type(map.get("@type"))
                || map.contains_key("telephone")
                || map.contains_key("address")
            {
                push_all(&mut found.names, strings(map.get("name")));
                push_all(&mut found.telephones, strings(map.get("telephone")));
                push_all(
                    &mut found.emails,
                    strings(map.get("email"))
                        .into_iter()
                        .map(|e| e.trim_start_matches("mailto:").to_ascii_lowercase())
                        .collect(),
                );
                push_all(&mut found.descriptions, strings(map.get("description")));
                let addresses: Vec<String> = match map.get("address") {
                    Some(Value::Array(items)) => items.iter().filter_map(postal_address).collect(),
                    Some(other) => postal_address(other).into_iter().collect(),
                    None => Vec::new(),
                };
                push_all(&mut found.addresses, addresses);
                push_all(
                    &mut found.same_as,
                    strings(map.get("sameAs"))
                        .into_iter()
                        .filter_map(|url| web_url(base, &url))
                        .collect(),
                );
                let logos: Vec<String> = match map.get("logo") {
                    Some(Value::Object(image)) => strings(image.get("url")),
                    other => strings(other),
                };
                push_all(
                    &mut found.logos,
                    logos
                        .into_iter()
                        .filter_map(|url| web_url(base, &url))
                        .collect(),
                );
            }
            for (key, nested) in map {
                if key == "@graph" || nested.is_object() || nested.is_array() {
                    read_json_ld(nested, base, found, depth + 1);
                }
            }
        }
        _ => {}
    }
}

/// Converts a DOM subtree to compact Markdown.
struct MarkdownWriter<'a> {
    base: &'a Url,
    out: String,
    list_depth: usize,
    words: u32,
}

const SKIPPED: &[&str] = &[
    "script", "style", "noscript", "template", "svg", "iframe", "canvas", "head", "select",
    "option", "object", "embed", "video", "audio", "map", "math", "input", "textarea",
];
const BLOCKS: &[&str] = &[
    "p",
    "div",
    "section",
    "article",
    "header",
    "footer",
    "nav",
    "main",
    "aside",
    "address",
    "figure",
    "figcaption",
    "form",
    "fieldset",
    "table",
    "tr",
    "dl",
    "dt",
    "dd",
    "blockquote",
    "pre",
    "details",
    "summary",
];

impl<'a> MarkdownWriter<'a> {
    fn new(base: &'a Url) -> Self {
        Self {
            base,
            out: String::new(),
            list_depth: 0,
            words: 0,
        }
    }

    fn full(&self) -> bool {
        self.out.len() >= MAX_MARKDOWN_CHARS
    }

    fn block_break(&mut self) {
        let trimmed = self.out.trim_end_matches(' ').len();
        self.out.truncate(trimmed);
        if !self.out.is_empty() && !self.out.ends_with("\n\n") {
            if self.out.ends_with('\n') {
                self.out.push('\n');
            } else {
                self.out.push_str("\n\n");
            }
        }
    }

    fn line_break(&mut self) {
        let trimmed = self.out.trim_end_matches(' ').len();
        self.out.truncate(trimmed);
        if !self.out.is_empty() && !self.out.ends_with('\n') {
            self.out.push('\n');
        }
    }

    fn text(&mut self, raw: &str) {
        let text = collapse(raw);
        if text.is_empty() {
            if !raw.is_empty() && !self.out.ends_with([' ', '\n']) && !self.out.is_empty() {
                self.out.push(' ');
            }
            return;
        }
        self.words += text.split(' ').count() as u32;
        let leading = raw.starts_with(char::is_whitespace);
        if leading && !self.out.ends_with([' ', '\n']) && !self.out.is_empty() {
            self.out.push(' ');
        }
        self.out.push_str(&text);
        if raw.ends_with(char::is_whitespace) {
            self.out.push(' ');
        }
    }

    fn walk(&mut self, element: ElementRef<'_>, depth: usize) {
        if depth > MAX_DEPTH || self.full() {
            return;
        }
        let name = element.value().name();
        if SKIPPED.contains(&name) || is_hidden(element) {
            return;
        }
        if let Some(email) = cloudflare_email(element) {
            self.words += 1;
            if !self.out.ends_with([' ', '\n']) && !self.out.is_empty() {
                self.out.push(' ');
            }
            self.out.push_str(&format!("[{email}](mailto:{email}) "));
            return;
        }
        match name {
            "br" => return self.line_break(),
            "hr" => {
                self.block_break();
                self.out.push_str("---");
                return self.block_break();
            }
            "img" => return,
            "a" => return self.anchor(element),
            _ => {}
        }
        let heading = match name {
            "h1" => Some(1),
            "h2" => Some(2),
            "h3" => Some(3),
            "h4" => Some(4),
            "h5" => Some(5),
            "h6" => Some(6),
            _ => None,
        };
        if let Some(level) = heading {
            self.block_break();
            self.out.push_str(&"#".repeat(level));
            self.out.push(' ');
        } else if name == "li" {
            self.line_break();
            self.out
                .push_str(&"  ".repeat(self.list_depth.saturating_sub(1)));
            self.out.push_str("- ");
        } else if name == "ul" || name == "ol" {
            self.block_break();
            self.list_depth += 1;
        } else if BLOCKS.contains(&name) {
            self.block_break();
        }
        self.children(element, depth);
        if name == "ul" || name == "ol" {
            self.list_depth = self.list_depth.saturating_sub(1);
            self.block_break();
        } else if heading.is_some() || BLOCKS.contains(&name) {
            self.block_break();
        } else if name == "li" {
            self.line_break();
        } else if matches!(name, "td" | "th") {
            self.out.push_str(" | ");
        } else if name == "button" && !self.out.ends_with([' ', '\n']) {
            self.out.push(' ');
        }
    }

    fn children(&mut self, element: ElementRef<'_>, depth: usize) {
        for child in element.children() {
            match child.value() {
                Node::Text(text) => self.text(text),
                Node::Element(_) => {
                    if let Some(child) = ElementRef::wrap(child) {
                        self.walk(child, depth + 1);
                    }
                }
                _ => {}
            }
        }
    }

    fn anchor(&mut self, element: ElementRef<'_>) {
        let href = element.value().attr("href").unwrap_or("").trim();
        let lower = href.to_ascii_lowercase();
        let target = if lower.starts_with("tel:") || lower.starts_with("mailto:") {
            Some(href.split_whitespace().collect::<String>())
        } else {
            web_url(self.base, href)
        };
        let label = link_label(element);
        self.words += label.split_whitespace().count() as u32;
        if !self.out.ends_with([' ', '\n']) && !self.out.is_empty() {
            self.out.push(' ');
        }
        match target {
            Some(target) => {
                let label = if label.is_empty() {
                    "link".to_string()
                } else {
                    label.replace(['[', ']'], "")
                };
                self.out.push_str(&format!("[{label}]({target})"));
            }
            None => self.out.push_str(&label),
        }
        self.out.push(' ');
    }

    fn finish(self) -> String {
        let mut lines: Vec<&str> = Vec::new();
        let mut blank = false;
        for line in self.out.lines() {
            let line = line.trim_end();
            if line.trim().is_empty() {
                if !blank && !lines.is_empty() {
                    lines.push("");
                }
                blank = true;
            } else {
                lines.push(line);
                blank = false;
            }
        }
        let mut markdown = lines.join("\n").trim().to_string();
        if markdown.len() > MAX_MARKDOWN_CHARS {
            let mut cut = MAX_MARKDOWN_CHARS;
            while !markdown.is_char_boundary(cut) {
                cut -= 1;
            }
            markdown.truncate(cut);
        }
        markdown
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn nesting_is_limited_without_touching_normal_pages() {
        let normal = "<html><body><div><p>one<p>two<ul><li>a<li>b</ul><script>if(a<b){'<div>'}</script></div></body></html>";
        assert_eq!(super::limit_nesting(normal, 10), normal);
        let deep = format!("<body>{}text", "<div>".repeat(50));
        let cut = super::limit_nesting(&deep, 10);
        assert_eq!(cut.matches("<div>").count(), 10);
        assert!(!cut.contains("text"));
        assert_eq!(super::limit_nesting("a < b <", 2), "a < b <");
    }

    use super::*;

    fn base() -> Url {
        Url::parse("https://shop.test/").unwrap()
    }

    #[test]
    fn reads_metadata_links_and_markdown() {
        let html = r##"<!doctype html><html lang="bn"><head>
            <title> Sample   Bakery </title>
            <meta name="description" content="Fresh  cakes">
            <meta name="theme-color" content="#e11d48">
            <meta property="og:site_name" content="Sample Bakery">
            <link rel="stylesheet" href="/s.css">
            <link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Hind+Siliguri:wght@400;700">
            <link rel="icon" href="/favicon.ico">
            <style>h1{color:#123456}</style>
            </head><body>
            <header><a class="site-logo" href="/"><img src="/logo.png" alt="Sample"></a>
            <nav><a href="/about">About us</a> <a href="/contact">যোগাযোগ</a></nav></header>
            <h1>Fresh cakes daily</h1><p>Call <a href="tel:+8801711223344">01711-223344</a></p>
            <ul><li>Cakes</li><li>Breads</li></ul>
            <a href="https://facebook.com/SampleBakery" aria-label="Facebook"><i class="fa fa-facebook"></i></a>
            <script>var x = "<p>not text</p>";</script>
            </body></html>"##;
        let page = analyze_page(html, &base());
        assert_eq!(page.title.as_deref(), Some("Sample Bakery"));
        assert_eq!(page.description.as_deref(), Some("Fresh cakes"));
        assert_eq!(page.lang.as_deref(), Some("bn"));
        assert_eq!(page.theme_color.as_deref(), Some("#E11D48"));
        assert_eq!(page.stylesheet_urls, vec!["https://shop.test/s.css"]);
        assert_eq!(
            page.google_fonts.get("Hind Siliguri"),
            Some(&vec![400, 700])
        );
        assert_eq!(page.inline_styles.len(), 1);
        assert_eq!(page.logo_candidates[0].reason, LogoReason::HeaderLogo);
        assert_eq!(
            page.logo_candidates[0].url.as_deref(),
            Some("https://shop.test/logo.png")
        );
        assert_eq!(
            page.logo_candidates.last().map(|c| c.reason),
            Some(LogoReason::Icon)
        );
        assert!(page
            .links
            .iter()
            .any(|l| l.text == "যোগাযোগ" && l.url == "https://shop.test/contact"));
        assert!(page.markdown.contains("# Fresh cakes daily"));
        assert!(page.markdown.contains("[01711-223344](tel:+8801711223344)"));
        assert!(page.markdown.contains("- Cakes\n- Breads"));
        assert!(page
            .markdown
            .contains("[Facebook](https://facebook.com/SampleBakery)"));
        assert!(!page.markdown.contains("not text"));
    }

    #[test]
    fn keeps_inline_svg_logos_as_standalone_documents() {
        let html = r#"<html><body><header><a class="logo" href="/"><svg viewBox="0 0 10 10"><linearGradient id="g"></linearGradient><path d="M0 0h10" fill="url(#g)"/></svg></a></header></body></html>"#;
        let page = analyze_page(html, &base());
        let svg = page.logo_candidates[0].svg.as_deref().unwrap();
        assert!(svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""));
        assert!(svg.contains("viewBox=\"0 0 10 10\""));
        assert!(svg.contains("linearGradient"));
    }

    #[test]
    fn decodes_cloudflare_protected_emails_and_records_class_roles() {
        assert_eq!(
            decode_cloudflare_email("076264686a6a6275646247606269736b627766756c2964686a")
                .as_deref(),
            Some("ecommerce@gentlepark.com")
        );
        let html = r#"<html><body class="font-sans"><h1 class="font-display">Hi</h1>
            <p>Mail <a href="/cdn-cgi/l/email-protection#076264686a6a6275646247606269736b627766756c2964686a"><span class="__cf_email__" data-cfemail="076264686a6a6275646247606269736b627766756c2964686a">[email&#160;protected]</span></a></p>
            <a class="btn-primary" href="/order">Order</a></body></html>"#;
        let page = analyze_page(html, &base());
        assert!(page.markdown.contains("ecommerce@gentlepark.com"));
        assert_eq!(page.class_roles.get("font-sans"), Some(&vec!["body"]));
        assert_eq!(
            page.class_roles.get("font-display"),
            Some(&vec!["headings"])
        );
        assert_eq!(page.class_roles.get("btn-primary"), Some(&vec!["buttons"]));
    }

    #[test]
    fn detects_javascript_shells() {
        let shell = r#"<html><body><noscript>You need to enable JavaScript to run this app.</noscript><div id="root"></div><script src="/main.js"></script></body></html>"#;
        assert!(analyze_page(shell, &base()).looks_like_js_shell());
        let next = format!(
            r#"<html><body><div id="__next"><h1>Shop</h1><p>{}</p></div><script src="/a.js"></script></body></html>"#,
            "word ".repeat(300)
        );
        assert!(!analyze_page(&next, &base()).looks_like_js_shell());
    }

    #[test]
    fn reads_schema_org_business_facts() {
        let html = r#"<html><head><script type="application/ld+json">
            {"@context":"https://schema.org","@graph":[{"@type":["LocalBusiness","Bakery"],
             "name":"Sample Bakery","telephone":["+880 1711-223344"],"email":"mailto:Hello@Bakery.test",
             "address":{"@type":"PostalAddress","streetAddress":"12 Road 5","addressLocality":"Dhaka","postalCode":"1209","addressCountry":{"name":"Bangladesh"}},
             "sameAs":["https://www.facebook.com/SampleBakery"],"logo":{"@type":"ImageObject","url":"/logo.svg"}}]}
            </script></head><body></body></html>"#;
        let page = analyze_page(html, &base());
        let found = &page.structured;
        assert_eq!(found.names, vec!["Sample Bakery"]);
        assert_eq!(found.telephones, vec!["+880 1711-223344"]);
        assert_eq!(found.emails, vec!["hello@bakery.test"]);
        assert_eq!(found.addresses, vec!["12 Road 5, Dhaka, 1209, Bangladesh"]);
        assert_eq!(found.logos, vec!["https://shop.test/logo.svg"]);
    }

    #[test]
    fn survives_hostile_markup() {
        let deep = "<div>".repeat(5000) + "deep text" + &"</div>".repeat(5000);
        let page = analyze_page(&deep, &base());
        assert!(page.markdown.len() < 1000);
        let broken =
            "<html><body><a href='http://[::1'>x</a><img src='javascript:alert(1)' class=logo>";
        let page = analyze_page(broken, &base());
        assert!(page.links.is_empty());
        assert!(page.logo_candidates.is_empty());
    }

    fn logo_urls(html: &str) -> Vec<String> {
        analyze_page(html, &base())
            .logo_candidates
            .into_iter()
            .filter(|c| matches!(c.reason, LogoReason::HeaderLogo | LogoReason::LogoHint))
            .filter_map(|c| c.url)
            .collect()
    }

    #[test]
    fn logo_hints_match_whole_words_only() {
        let html = r#"<body>
            <div class="client-logos"><img src="/c1.png"></div>
            <div class="partner-logo"><img src="/p1.png" alt="Partner"></div>
            <a class="logout" href="/out"><img src="/out.png"></a>
            <img src="/img/clients/logo.png">
            <img src="/img/catalog.png" class="logos">
            <section><img class="footer-logo" src="/footer.png"></section>
            <header><a href="/"><img class="site-logo" src="/site.png"></a></header>
            <img class="navbar-logo" src="/nav.png"></body>"#;
        assert_eq!(
            logo_urls(html),
            vec![
                "https://shop.test/site.png",
                "https://shop.test/footer.png",
                "https://shop.test/nav.png",
            ]
        );
    }

    #[test]
    fn home_linked_logos_come_first() {
        let html = r#"<body><img class="logo" src="/other.png"><a href="/"><img class="logo" src="/home.png"></a></body>"#;
        assert_eq!(logo_urls(html)[0], "https://shop.test/home.png");
    }

    #[test]
    fn honours_base_href() {
        let html = r#"<html><head><base href="https://cdn.shop.test/v2/"><link rel="stylesheet" href="app.css"></head><body><a href="contact">Contact</a><base href="https://evil.test/"></body></html>"#;
        let page = analyze_page(html, &base());
        assert_eq!(page.links[0].url, "https://cdn.shop.test/v2/contact");
        assert_eq!(
            page.stylesheet_urls,
            vec!["https://cdn.shop.test/v2/app.css"]
        );
        let bad = r#"<head><base href="javascript:alert(1)"></head><a href="/x">x</a>"#;
        assert_eq!(
            analyze_page(bad, &base()).links[0].url,
            "https://shop.test/x"
        );
    }

    #[test]
    fn accepts_prefixed_and_broader_schema_types() {
        let html = r#"<script type="application/ld+json">[
            {"@type":"schema:Dentist","name":"Smile","telephone":"+8801711223344"},
            {"@type":"http://schema.org/Hotel","name":"Stay"},
            {"@type":"WebPage","name":"Home page","address":"Dhaka"},
            {"@type":"Person","name":"Not a business"}]</script>"#;
        let found = analyze_page(html, &base()).structured;
        assert_eq!(found.names, vec!["Smile", "Stay", "Home page"]);
        assert_eq!(found.telephones, vec!["+8801711223344"]);
        assert_eq!(found.addresses, vec!["Dhaka"]);
    }

    #[test]
    fn inline_svg_logos_are_rebuilt_from_an_allow_list() {
        let html = r##"<header><a class="logo" href="/"><svg viewBox="0 0 10 10" onload="alert(1)" width="10">
            <script>alert(1)</script>
            <foreignObject><iframe src="https://evil.test"></iframe></foreignObject>
            <a href="javascript:alert(1)"><path d="M0 0h5" onclick="x()"/></a>
            <path d="M0 0h10" fill="url(#g)" style="fill:red" onmouseover="x()"/>
            <path d="M1 1" fill="url(https://evil.test/x.svg#a)"/>
            <path d="M2 2" style="fill:url('http://evil.test/a.png')"/>
            <path d="M3 3" style="background:u\72l(http://evil.test)"/>
            <image href="https://evil.test/a.png"/>
            <use href="https://evil.test/a.svg#x"/><use xlink:href="#ok" href="#ok"/>
            <style>.a{fill:red}</style><style>.b{fill:url(http://evil.test/i.png)}</style>
            <style>@import url(http://evil.test/x.css);</style>
            <text x="1" title='"><script>'>Hi &lt;b&gt; &amp; co</text>
            <linearGradient id="g"><stop offset="0" stop-color="#fff"/></linearGradient>
            </svg></a></header>"##;
        let page = analyze_page(html, &base());
        let svg = page.logo_candidates[0].svg.as_deref().unwrap();
        for bad in [
            "script",
            "alert",
            "onload",
            "onclick",
            "onmouseover",
            "foreignObject",
            "iframe",
            "evil.test",
            "javascript",
            "<image",
            "@import",
            "<a ",
            "title=",
        ] {
            assert!(!svg.contains(bad), "{bad} in {svg}");
        }
        assert!(
            svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""),
            "{svg}"
        );
        assert!(svg.contains("viewBox=\"0 0 10 10\""));
        assert!(svg.contains("fill=\"url(#g)\""));
        assert!(svg.contains("href=\"#ok\""));
        assert!(svg.contains(".a{fill:red}"));
        assert!(svg.contains("Hi &lt;b&gt; &amp; co"));
        assert!(svg.contains("<linearGradient id=\"g\"><stop"));
        // The result is well-formed XML.
        let mut reader = quick_xml::Reader::from_str(svg);
        loop {
            match reader.read_event() {
                Ok(quick_xml::events::Event::Eof) => break,
                Ok(_) => {}
                Err(e) => panic!("not well-formed: {e}: {svg}"),
            }
        }
    }
}
