//! DOM cleaning: removes scripts, hidden text, tracking pixels, consent banners and (in
//! main-content mode) page chrome such as navigation, sidebars and "related" blocks.
//!
//! Hidden text is removed in every mode. It is the usual place for instructions planted for
//! AI agents, so dropping it is both cleaner output and a safety measure.

use crate::error::{SeoError, SeoResult};
use dom_query::{Document, NodeRef};
use regex::Regex;
use std::sync::OnceLock;

/// Elements that never carry readable content.
const JUNK_TAGS: &str = "script, style, noscript, template, iframe, object, embed, svg, canvas, \
    link, meta, button, input, select, option, textarea, dialog, video, audio, source, track, \
    map, area, frame, frameset, applet";

/// Elements hidden by attribute or ARIA.
const HIDDEN_SELECTORS: &str =
    "[hidden], [data-bs-hidden], [aria-hidden='true'], [role='dialog'], \
    [aria-modal='true'], .sr-only, .visually-hidden, .screen-reader-text, .hidden, .d-none, \
    .invisible";

fn compiled(cell: &'static OnceLock<Option<Regex>>, pattern: &str) -> Option<&'static Regex> {
    cell.get_or_init(|| Regex::new(pattern).ok()).as_ref()
}

fn hidden_style_re() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    compiled(
        &RE,
        r"(?ix)
        display \s* : \s* none
        | visibility \s* : \s* (hidden|collapse)
        | font-size \s* : \s* 0 (\.0+)? \s* (px|em|rem|pt|%)? \s* (;|$|!)
        | opacity \s* : \s* 0 (\.0+)? \s* (;|$|!)
        | (left|top|text-indent) \s* : \s* -\d{3,}
        | clip \s* : \s* rect\(\s*0
        ",
    )
}

fn consent_re() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    compiled(
        &RE,
        r"(?i)^(cookie[-_]?(banner|consent|notice|bar|law|popup|policy-banner|wall)|cookiebar|cookies?-?modal|cc[-_](window|banner)|onetrust.*|optanon.*|cybotcookiebot.*|truste.*|consent[-_]?(banner|manager|modal|popup)?|gdpr.*|cmp[-_]?(banner|container))$",
    )
}

fn boilerplate_re() -> Option<&'static Regex> {
    static RE: OnceLock<Option<Regex>> = OnceLock::new();
    compiled(
        &RE,
        r"(?i)^(related|related[-_]?(posts?|articles?|products?|content|links|stories|items)|you[-_]?may[-_]?also[-_]?like|share|sharing|share[-_]?(buttons?|links|bar|tools)|social|social[-_]?(links|share|icons|media)|breadcrumbs?|sidebar|side[-_]?bar|widget|widget[-_]?area|newsletter|subscribe|subscription|signup|sign[-_]?up|advert|advertisement|ads?|ad[-_]?(slot|container|banner|unit|wrapper)|sponsored|promo|popup|modal|toolbar|pagination|pager|skip[-_]?link|skip[-_]?to[-_]?content|comments?|comment[-_]?(list|section|form|area)|disqus|author[-_]?bio|top[-_]?bar|site[-_]?header|site[-_]?footer|masthead|menu|navbar|nav|navigation|footer|header|reply[-_]?box|cookie.*|consent.*|gdpr.*|filters?|facets?)$",
    )
}

/// Cleaning settings.
#[derive(Debug, Clone, Copy)]
pub struct CleanOptions<'a> {
    /// Also strip page chrome (navigation, header, footer, sidebars, related blocks).
    pub only_main_content: bool,
    /// Caller-supplied CSS selectors to remove.
    pub exclude_selectors: &'a [String],
}

/// Selects with a caller-supplied CSS selector, turning a bad selector into a config error.
pub fn try_select<'a>(doc: &'a Document, selector: &str) -> SeoResult<dom_query::Selection<'a>> {
    let matcher = dom_query::Matcher::new(selector)
        .map_err(|_| SeoError::Config(format!("Invalid CSS selector '{selector}'")))?;
    Ok(doc.select_matcher(&matcher))
}

/// Cleans the document in place.
pub fn clean_document(doc: &Document, opts: CleanOptions<'_>) -> SeoResult<()> {
    for selector in opts.exclude_selectors {
        try_select(doc, selector)?.remove();
    }

    remove_matching(doc, JUNK_TAGS, |node| {
        // Keep head metadata; only strip it from the body.
        !matches!(node_name(node).as_str(), "link" | "meta") || in_body(node)
    });
    remove_matching(doc, "input[type=hidden]", |_| true);
    remove_matching(doc, HIDDEN_SELECTORS, |node| {
        !is_root(node) && (node.has_attr("data-bs-hidden") || !shown_at_some_breakpoint(node))
    });
    remove_matching(doc, "[style]", |node| {
        !is_root(node)
            && node
                .attr("style")
                .is_some_and(|style| hidden_style_re().is_some_and(|re| re.is_match(&style)))
    });

    // Tracking pixels and inline base64 images.
    remove_matching(doc, "img", |node| {
        let src = node.attr("src").map(|s| s.to_string()).unwrap_or_default();
        let tiny = |attr: &str| {
            node.attr(attr)
                .and_then(|v| v.trim().trim_end_matches("px").parse::<u32>().ok())
                .is_some_and(|v| v <= 1)
        };
        src.trim_start().starts_with("data:") || (tiny("width") && tiny("height"))
    });

    // Consent banners in every mode.
    remove_matching(doc, "[id], [class]", |node| {
        !is_root(node) && tokens_match(node, consent_re())
    });

    if opts.only_main_content {
        remove_matching(
            doc,
            "nav, aside, [role=navigation], [role=complementary], [role=search], form[role=search]",
            |node| !contains_main(node),
        );
        // Page-level header/footer and banners; an <article>'s own header keeps its title.
        remove_matching(
            doc,
            "header, footer, [role=banner], [role=contentinfo]",
            |node| !inside_content_root(node) && !contains_main(node),
        );
        remove_matching(doc, "[id], [class]", |node| {
            !is_root(node)
                && !matches!(node_name(node).as_str(), "main" | "article")
                && !contains_main(node)
                && tokens_match(node, boilerplate_re())
        });
    }
    Ok(())
}

fn remove_matching(doc: &Document, selector: &str, keep_if_false: impl Fn(&NodeRef) -> bool) {
    let selection = doc.select(selector);
    let doomed: Vec<NodeRef> = selection
        .nodes()
        .iter()
        .filter(|n| keep_if_false(n))
        .cloned()
        .collect();
    for node in doomed {
        node.remove_from_parent();
    }
}

pub(crate) fn node_name(node: &NodeRef) -> String {
    node.node_name()
        .map(|n| n.to_ascii_lowercase())
        .unwrap_or_default()
}

fn is_root(node: &NodeRef) -> bool {
    matches!(node_name(node).as_str(), "html" | "body" | "head")
}

fn in_body(node: &NodeRef) -> bool {
    node.ancestors_it(None).any(|a| node_name(&a) == "body")
}

fn inside_content_root(node: &NodeRef) -> bool {
    node.ancestors_it(None).any(|a| {
        matches!(node_name(&a).as_str(), "article" | "main")
            || a.attr("role")
                .is_some_and(|r| r.eq_ignore_ascii_case("main"))
    })
}

fn contains_main(node: &NodeRef) -> bool {
    node.descendants_it().any(|d| {
        matches!(node_name(&d).as_str(), "main" | "article" | "h1")
            || d.attr("role")
                .is_some_and(|r| r.eq_ignore_ascii_case("main"))
    })
}

fn tokens_match(node: &NodeRef, re: Option<&Regex>) -> bool {
    let Some(re) = re else {
        return false;
    };
    let class = node
        .attr("class")
        .map(|c| c.to_string())
        .unwrap_or_default();
    let id = node.attr("id").map(|c| c.to_string()).unwrap_or_default();
    class
        .split_ascii_whitespace()
        .chain(std::iter::once(id.trim()))
        .filter(|t| !t.is_empty())
        .any(|t| re.is_match(t))
}

/// Utility-class frameworks hide elements on small screens only (`hidden md:block`,
/// `d-none d-lg-flex`); such elements are visible on desktop and are kept.
fn shown_at_some_breakpoint(node: &NodeRef) -> bool {
    let class = node
        .attr("class")
        .map(|c| c.to_string())
        .unwrap_or_default();
    class.split_ascii_whitespace().any(|t| {
        (t.contains(':') && !t.ends_with(":hidden"))
            || (t.starts_with("d-") && t != "d-none" && t.matches('-').count() >= 2)
    })
}
