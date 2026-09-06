//! # Developer Page Inspector Renderer
//!
//! ANSI-formatted single-page Developer X-Ray visualization displaying
//! core metadata, OpenGraph, Twitter Cards, JSON-LD schemas, headings hierarchy,
//! and link & asset telemetry.

use crate::core::models::{IssueFinding, RobotsFlags};
use crate::crawler::client::FetchResult;
use crate::parser::ParsedPage;

const ANSI_CYAN: &str = "\x1b[38;5;51m";
const ANSI_GREEN: &str = "\x1b[38;5;48m";
const ANSI_RED: &str = "\x1b[38;5;196m";
const ANSI_YELLOW: &str = "\x1b[38;5;220m";
const ANSI_DIM: &str = "\x1b[38;5;244m";
const ANSI_BRIGHT_WHITE: &str = "\x1b[38;5;231m";
const ANSI_BOLD: &str = "\x1b[1m";
const ANSI_RESET: &str = "\x1b[0m";

/// Formats the complete ANSI developer inspection view for a single page.
pub fn format_page_inspection(
    page: &ParsedPage,
    fetch: &FetchResult,
    _issues: &[IssueFinding],
) -> String {
    let mut out = String::with_capacity(4096);

    // 1. ASCII Banner (no extra subheadings below it)
    out.push_str(&format!(
        "\n{ANSI_CYAN}{ANSI_BOLD}  ███████╗███████╗ ██████╗     ██╗     ███████╗███╗   ██╗███████╗\n  ██╔════╝██╔════╝██╔═══██╗    ██║     ██╔════╝████╗  ██║██╔════╝\n  ███████╗█████╗  ██║   ██║    ██║     █████╗  ██╔██╗ ██║███████╗\n  ╚════██║██╔══╝  ██║   ██║    ██║     ██╔══╝  ██║╚██╗██║╚════██║\n  ███████║███████╗╚██████╔╝    ███████╗███████╗██║ ╚████║███████║\n  ╚══════╝╚══════╝ ╚═════╝     ╚══════╝╚══════╝╚═╝  ╚═══╝╚══════╝{ANSI_RESET}\n\n"
    ));

    // 2. Protocol & Target Telemetry
    let status_color = if fetch.status_code == 200 {
        ANSI_GREEN
    } else if fetch.status_code >= 300 && fetch.status_code < 400 {
        ANSI_YELLOW
    } else {
        ANSI_RED
    };

    let redirect_info = if fetch.final_url != fetch.url {
        format!(
            " {ANSI_YELLOW}⚡ Redirected (+{} hops){ANSI_RESET}",
            fetch.redirect_chain.len().max(1)
        )
    } else {
        format!(" {ANSI_DIM}(No redirects){ANSI_RESET}")
    };

    let size_kb = (fetch.size_bytes as f64) / 1024.0;

    let has_https = fetch.url.starts_with("https://");
    let has_hsts = fetch.headers.contains_key("strict-transport-security");
    let has_csp = fetch.headers.contains_key("content-security-policy");
    let has_x_frame = fetch.headers.contains_key("x-frame-options");
    let has_x_content = fetch.headers.contains_key("x-content-type-options");

    let check = |b: bool| {
        if b {
            format!("{ANSI_GREEN}✔{ANSI_RESET}")
        } else {
            format!("{ANSI_RED}✘{ANSI_RESET}")
        }
    };

    out.push_str(&format_section_header("TARGET & PROTOCOL TELEMETRY"));
    out.push_str(&format!(
        "  {ANSI_BOLD}Request URL  {ANSI_RESET}: {ANSI_CYAN}{}{ANSI_RESET}\n",
        fetch.url
    ));
    out.push_str(&format!(
        "  {ANSI_BOLD}Final URL    {ANSI_RESET}: {ANSI_CYAN}{}{ANSI_RESET}{redirect_info}\n",
        fetch.final_url
    ));
    out.push_str(&format!(
        "  {ANSI_BOLD}Status Code  {ANSI_RESET}: {status_color}{ANSI_BOLD}{} {}{ANSI_RESET}\n",
        fetch.status_code,
        status_text(fetch.status_code)
    ));
    out.push_str(&format!(
        "  {ANSI_BOLD}TTFB / Size  {ANSI_RESET}: {}ms {ANSI_DIM}│{ANSI_RESET} {:.1} KB ({} bytes)\n",
        fetch.ttfb_ms, size_kb, fetch.size_bytes
    ));
    out.push_str(&format!(
        "  {ANSI_BOLD}Content-Type {ANSI_RESET}: {}\n",
        fetch.content_type
    ));
    out.push_str(&format!(
        "  {ANSI_BOLD}Security     {ANSI_RESET}: HTTPS: {} {ANSI_DIM}│{ANSI_RESET} HSTS: {} {ANSI_DIM}│{ANSI_RESET} CSP: {} {ANSI_DIM}│{ANSI_RESET} X-Frame: {} {ANSI_DIM}│{ANSI_RESET} X-Content-Type: {}\n",
        check(has_https),
        check(has_hsts),
        check(has_csp),
        check(has_x_frame),
        check(has_x_content),
    ));

    let archetype_badge = page.page_intent.archetype.badge();
    let conf_pct = (page.page_intent.confidence * 100.0).round() as u32;
    out.push_str(&format!(
        "  {ANSI_BOLD}Archetype    {ANSI_RESET}: {ANSI_CYAN}{ANSI_BOLD}[{archetype_badge}]{ANSI_RESET} {ANSI_DIM}({conf_pct}% confidence){ANSI_RESET}\n",
    ));

    // 3. Core Metadata & Directives (with generous vertical spacing before header)
    out.push_str(&format!(
        "\n\n{}",
        format_section_header("CORE METADATA & DIRECTIVES")
    ));

    match &page.title {
        Some(t) => {
            let len = t.len();
            let badge = if (30..=60).contains(&len) {
                format!("{ANSI_GREEN}↳ {len} chars ✔ (Optimal: 30–60 chars){ANSI_RESET}")
            } else if len < 30 {
                format!("{ANSI_YELLOW}↳ {len} chars ⚠️ (Below recommended 30 chars){ANSI_RESET}")
            } else {
                format!("{ANSI_YELLOW}↳ {len} chars ⚠️ (Above recommended 60 chars){ANSI_RESET}")
            };
            let wrapped = wrap_text(t, 58);
            if wrapped.len() <= 1 {
                out.push_str(&format!(
                    "  {ANSI_BOLD}Title        {ANSI_RESET}: \"{}\"\n                {badge}\n",
                    wrapped.first().map(|s| s.as_str()).unwrap_or("")
                ));
            } else {
                out.push_str(&format!(
                    "  {ANSI_BOLD}Title        {ANSI_RESET}: \"{}\n",
                    wrapped[0]
                ));
                for line in &wrapped[1..wrapped.len() - 1] {
                    out.push_str(&format!("                 {}\n", line));
                }
                out.push_str(&format!(
                    "                 {}\"\n                {badge}\n",
                    wrapped.last().map(|s| s.as_str()).unwrap_or("")
                ));
            }
        }
        None => {
            out.push_str(&format!("  {ANSI_BOLD}Title        {ANSI_RESET}: {ANSI_RED}(Missing document title tag!) 🚨{ANSI_RESET}\n"));
        }
    }

    match &page.meta_description {
        Some(d) => {
            let len = d.len();
            let badge = if (70..=160).contains(&len) {
                format!("{ANSI_GREEN}↳ {len} chars ✔ (Optimal: 70–160 chars){ANSI_RESET}")
            } else if len < 70 {
                format!("{ANSI_YELLOW}↳ {len} chars ⚠️ (Below recommended 70 chars){ANSI_RESET}")
            } else {
                format!("{ANSI_YELLOW}↳ {len} chars ⚠️ (Above recommended 160 chars){ANSI_RESET}")
            };
            let wrapped = wrap_text(d, 58);
            if wrapped.len() <= 1 {
                out.push_str(&format!(
                    "  {ANSI_BOLD}Description  {ANSI_RESET}: \"{}\"\n                {badge}\n",
                    wrapped.first().map(|s| s.as_str()).unwrap_or("")
                ));
            } else {
                out.push_str(&format!(
                    "  {ANSI_BOLD}Description  {ANSI_RESET}: \"{}\n",
                    wrapped[0]
                ));
                for line in &wrapped[1..wrapped.len() - 1] {
                    out.push_str(&format!("                 {}\n", line));
                }
                out.push_str(&format!(
                    "                 {}\"\n                {badge}\n",
                    wrapped.last().map(|s| s.as_str()).unwrap_or("")
                ));
            }
        }
        None => {
            out.push_str(&format!("  {ANSI_BOLD}Description  {ANSI_RESET}: {ANSI_YELLOW}(Missing meta description) ⚠️{ANSI_RESET}\n"));
        }
    }

    match &page.canonical_url {
        Some(c) => {
            let is_self = c == &fetch.final_url || c == &fetch.url;
            if is_self {
                out.push_str(&format!(
                    "  {ANSI_BOLD}Canonical    {ANSI_RESET}: {ANSI_CYAN}{c}{ANSI_RESET}\n                {ANSI_GREEN}↳ Self-referencing ✔{ANSI_RESET}\n"
                ));
            } else {
                out.push_str(&format!(
                    "  {ANSI_BOLD}Canonical    {ANSI_RESET}: {ANSI_CYAN}{c}{ANSI_RESET}\n                {ANSI_YELLOW}↳ ⚡ Points to alternate URL{ANSI_RESET}\n"
                ));
            }
        }
        None => {
            out.push_str(&format!("  {ANSI_BOLD}Canonical    {ANSI_RESET}: {ANSI_YELLOW}(Missing canonical URL tag) ⚠️{ANSI_RESET}\n"));
        }
    }

    let lang_str = page.html_lang.as_deref().unwrap_or("none");
    let charset_str = page.charset.as_deref().unwrap_or("none");
    let viewport_str = page.viewport.as_deref().unwrap_or("missing ⚠️");
    let robots_str = format_robots_flags(page.robots_flags);

    out.push_str(&format!(
        "  {ANSI_BOLD}Language/Set {ANSI_RESET}: {lang_str} {ANSI_DIM}│{ANSI_RESET} {charset_str}\n"
    ));
    out.push_str(&format!(
        "  {ANSI_BOLD}Viewport     {ANSI_RESET}: {viewport_str}\n"
    ));
    out.push_str(&format!(
        "  {ANSI_BOLD}Robots Flags {ANSI_RESET}: {robots_str}\n"
    ));
    out.push_str(&format!(
        "  {ANSI_BOLD}Content Stats{ANSI_RESET}: {} words {ANSI_DIM}│{ANSI_RESET} SimHash: 0x{:016x}\n",
        page.word_count, page.simhash,
    ));

    // 4. Social Cards & Open Graph (with generous vertical spacing before header)
    out.push_str(&format!(
        "\n\n{}",
        format_section_header("SOCIAL CARDS & OPEN GRAPH")
    ));

    let has_og = !page.open_graph.is_empty();
    let has_twitter = !page.twitter_cards.is_empty();

    if !has_og && !has_twitter {
        out.push_str(&format!(
            "  {ANSI_DIM}(No Open Graph or Twitter Card tags detected on this page){ANSI_RESET}\n"
        ));
    } else {
        if has_og {
            out.push_str(&format!(
                "  {ANSI_BOLD}{ANSI_CYAN}▸ Open Graph (Facebook / LinkedIn):{ANSI_RESET}\n"
            ));
            for (prop, val) in &page.open_graph {
                out.push_str(&format_wrapped_property(
                    "    ", ANSI_CYAN, prop, 16, val, 52,
                ));
            }
        }

        if has_twitter {
            if has_og {
                out.push('\n');
            }
            out.push_str(&format!(
                "  {ANSI_BOLD}{ANSI_YELLOW}▸ Twitter / X Card:{ANSI_RESET}\n"
            ));
            for (prop, val) in &page.twitter_cards {
                out.push_str(&format_wrapped_property(
                    "    ",
                    ANSI_YELLOW,
                    prop,
                    20,
                    val,
                    52,
                ));
            }
        }
    }

    // 5. Structured Data (JSON-LD) (with generous vertical spacing before header)
    out.push_str(&format!(
        "\n\n{}",
        format_section_header("STRUCTURED DATA (JSON-LD)")
    ));

    if page.schemas.is_empty() {
        out.push_str(&format!(
            "  {ANSI_DIM}(No JSON-LD structured data detected on this page){ANSI_RESET}\n"
        ));
    } else {
        out.push_str(&format!(
            "  {ANSI_BOLD}{ANSI_CYAN}▸ Discovered {ANSI_BRIGHT_WHITE}{}{ANSI_RESET}{ANSI_BOLD}{ANSI_CYAN} schema block(s):{ANSI_RESET}\n",
            page.schemas.len()
        ));
        for (i, schema) in page.schemas.iter().enumerate() {
            let rich_elig = if schema.is_google_eligible {
                format!("{ANSI_GREEN}Rich Result Eligible ✔{ANSI_RESET}")
            } else {
                format!("{ANSI_DIM}Standard Schema{ANSI_RESET}")
            };
            out.push_str(&format!(
                "  [{}] {ANSI_BOLD}@type: {}{ANSI_RESET} {ANSI_DIM}│{ANSI_RESET} {}\n",
                i + 1,
                schema.schema_type,
                rich_elig
            ));

            if !schema.missing_required_fields.is_empty() {
                let missing = schema
                    .missing_required_fields
                    .iter()
                    .map(|f| f.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push_str(&format!(
                    "      {ANSI_YELLOW}⚠️ Missing required fields:{ANSI_RESET} {}\n",
                    missing
                ));
            }

            // Extract key summary lines from raw JSON if valid
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&schema.raw_json) {
                if let Some(obj) = val.as_object() {
                    for key in ["name", "headline", "author", "datePublished", "description"] {
                        if let Some(v) = obj.get(key) {
                            let text = match v {
                                serde_json::Value::String(s) => s.clone(),
                                other => other.to_string(),
                            };
                            let truncated = if text.len() > 60 {
                                format!("{}...", &text[..57])
                            } else {
                                text
                            };
                            out.push_str(&format!(
                                "      ├── {:<13}: {}{ANSI_RESET}\n",
                                key, truncated
                            ));
                        }
                    }
                }
            }
        }
    }

    // 6. Headings Hierarchy (with generous vertical spacing before header)
    out.push_str(&format!(
        "\n\n{}",
        format_section_header("HEADINGS HIERARCHY")
    ));

    let has_subheadings = !page.h2_headings.is_empty() || !page.h3_headings.is_empty();
    let h1_sep = if has_subheadings { "\n\n" } else { "\n" };

    match &page.h1_primary {
        Some(h1) => {
            let h1_badge = if page.h1_count == 1 {
                format!("{ANSI_GREEN}(1 found ✔){ANSI_RESET}")
            } else {
                format!(
                    "{ANSI_RED}({} H1s found - duplicate! 🚨){ANSI_RESET}",
                    page.h1_count
                )
            };
            let wrapped = wrap_text(h1, 65);
            out.push_str(&format!(
                "  {ANSI_BOLD}{ANSI_CYAN}▸ H1 Primary Document Heading{ANSI_RESET} {h1_badge}:\n"
            ));
            if wrapped.len() <= 1 {
                out.push_str(&format!(
                    "    \"{}\"{h1_sep}",
                    wrapped.first().map(|s| s.as_str()).unwrap_or("")
                ));
            } else {
                out.push_str(&format!("    \"{}\n", wrapped[0]));
                for line in &wrapped[1..wrapped.len() - 1] {
                    out.push_str(&format!("     {}\n", line));
                }
                out.push_str(&format!(
                    "     {}\"{h1_sep}",
                    wrapped.last().map(|s| s.as_str()).unwrap_or("")
                ));
            }
        }
        None => {
            out.push_str(&format!(
                "  {ANSI_RED}▸ H1: (No primary H1 tag found!) 🚨{ANSI_RESET}{h1_sep}"
            ));
        }
    }

    if !page.h2_headings.is_empty() {
        out.push_str(&format!(
            "  {ANSI_BOLD}{ANSI_CYAN}▸ H2 Headings ({}) found:{ANSI_RESET}\n",
            page.h2_headings.len()
        ));
        for (idx, h2) in page.h2_headings.iter().take(12).enumerate() {
            let is_last = idx == page.h2_headings.len().min(12) - 1;
            let branch = if is_last { "└──" } else { "├──" };
            let wrapped = wrap_text(h2, 60);
            if wrapped.len() <= 1 {
                out.push_str(&format!(
                    "    {branch} {ANSI_CYAN}H2{ANSI_RESET}: {}\n",
                    wrapped.first().map(|s| s.as_str()).unwrap_or("")
                ));
            } else {
                out.push_str(&format!(
                    "    {branch} {ANSI_CYAN}H2{ANSI_RESET}: {}\n",
                    wrapped[0]
                ));
                for line in &wrapped[1..] {
                    out.push_str(&format!("            {}\n", line));
                }
            }
        }
        if page.h2_headings.len() > 12 {
            out.push_str(&format!(
                "    └── ... ({} more H2 headings)\n",
                page.h2_headings.len() - 12
            ));
        }
        if !page.h3_headings.is_empty() {
            out.push('\n');
        }
    }

    if !page.h3_headings.is_empty() {
        out.push_str(&format!(
            "  {ANSI_BOLD}{ANSI_DIM}▸ H3 Subheadings ({}) found:{ANSI_RESET}\n",
            page.h3_headings.len()
        ));
        for (idx, h3) in page.h3_headings.iter().take(12).enumerate() {
            let is_last = idx == page.h3_headings.len().min(12) - 1;
            let branch = if is_last { "└──" } else { "├──" };
            let wrapped = wrap_text(h3, 60);
            if wrapped.len() <= 1 {
                out.push_str(&format!(
                    "    {branch} {ANSI_DIM}H3:{ANSI_RESET} {}\n",
                    wrapped.first().map(|s| s.as_str()).unwrap_or("")
                ));
            } else {
                out.push_str(&format!(
                    "    {branch} {ANSI_DIM}H3:{ANSI_RESET} {}\n",
                    wrapped[0]
                ));
                for line in &wrapped[1..] {
                    out.push_str(&format!("            {}\n", line));
                }
            }
        }
        if page.h3_headings.len() > 12 {
            out.push_str(&format!(
                "    └── ... ({} more H3 headings)\n",
                page.h3_headings.len() - 12
            ));
        }
    }

    if page.h1_primary.is_none() && page.h2_headings.is_empty() && page.h3_headings.is_empty() {
        out.push_str(&format!(
            "  {ANSI_DIM}(No HTML headings detected on this page){ANSI_RESET}\n"
        ));
    }

    // 7. Links & Asset Inventory (with generous vertical spacing before header)
    out.push_str(&format!(
        "\n\n{}",
        format_section_header("LINKS & ASSET INVENTORY")
    ));
    let internal_links_count = page.links.iter().filter(|l| l.is_internal).count();
    let external_links_count = page.links.len() - internal_links_count;
    let missing_alt_count = page
        .images
        .iter()
        .filter(|img| img.alt_text.is_none())
        .count();
    let missing_alt_badge = if missing_alt_count == 0 {
        format!("{ANSI_GREEN}0 missing alt ✔{ANSI_RESET}")
    } else {
        format!("{ANSI_YELLOW}{missing_alt_count} missing alt ⚠️{ANSI_RESET}")
    };

    out.push_str(&format!(
        "  {ANSI_BOLD}Internal Links{ANSI_RESET}: {internal_links_count} links\n"
    ));
    out.push_str(&format!(
        "  {ANSI_BOLD}External Links{ANSI_RESET}: {external_links_count} external outbound links\n"
    ));
    out.push_str(&format!(
        "  {ANSI_BOLD}Images        {ANSI_RESET}: {} images ({missing_alt_badge})\n",
        page.images.len()
    ));
    out.push_str(&format!(
        "  {ANSI_BOLD}Hreflangs     {ANSI_RESET}: {} alternate language tag(s)\n\n",
        page.hreflangs.len()
    ));

    out
}

/// Prints the ANSI page inspection report directly to standard output.
pub fn print_page_inspection(page: &ParsedPage, fetch: &FetchResult, issues: &[IssueFinding]) {
    print!("{}", format_page_inspection(page, fetch, issues));
}

fn status_text(code: u16) -> &'static str {
    match code {
        200 => "OK",
        201 => "Created",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Unknown",
    }
}

fn format_robots_flags(flags: RobotsFlags) -> String {
    if flags.is_empty() || flags == RobotsFlags::NONE {
        return format!("{ANSI_GREEN}INDEX, FOLLOW{ANSI_RESET}");
    }

    let mut parts = Vec::new();
    if flags.contains(RobotsFlags::NOINDEX) {
        parts.push(format!("{ANSI_RED}NOINDEX{ANSI_RESET}"));
    } else {
        parts.push(format!("{ANSI_GREEN}INDEX{ANSI_RESET}"));
    }
    if flags.contains(RobotsFlags::NOFOLLOW) {
        parts.push(format!("{ANSI_RED}NOFOLLOW{ANSI_RESET}"));
    } else {
        parts.push(format!("{ANSI_GREEN}FOLLOW{ANSI_RESET}"));
    }
    if flags.contains(RobotsFlags::NOSNIPPET) {
        parts.push(format!("{ANSI_YELLOW}NOSNIPPET{ANSI_RESET}"));
    }
    if flags.contains(RobotsFlags::NOARCHIVE) {
        parts.push(format!("{ANSI_YELLOW}NOARCHIVE{ANSI_RESET}"));
    }
    if flags.contains(RobotsFlags::NOIMAGEINDEX) {
        parts.push(format!("{ANSI_YELLOW}NOIMAGEINDEX{ANSI_RESET}"));
    }

    parts.join(", ")
}

/// Splits `text` into lines where each line (joined with spaces) does not exceed `max_width`.
/// Words that exceed `max_width` on their own are kept intact on their own line.
fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current_line = String::new();

    for word in text.split_whitespace() {
        if current_line.is_empty() {
            current_line.push_str(word);
        } else if current_line.len() + 1 + word.len() <= max_width {
            current_line.push(' ');
            current_line.push_str(word);
        } else {
            lines.push(current_line);
            current_line = word.to_string();
        }
    }

    if !current_line.is_empty() {
        lines.push(current_line);
    }

    if lines.is_empty() {
        lines.push(String::new());
    }

    lines
}

fn format_wrapped_property(
    indent_prefix: &str,
    color: &str,
    prop: &str,
    key_width: usize,
    val: &str,
    max_line_width: usize,
) -> String {
    let visible_key_len = prop.len().max(key_width);
    let total_prefix_len = indent_prefix.len() + visible_key_len + 2; // +2 for ": "
    let cont_indent = " ".repeat(total_prefix_len);

    let wrapped = wrap_text(val, max_line_width);
    let mut out = String::new();
    if wrapped.len() <= 1 {
        out.push_str(&format!(
            "{indent_prefix}{color}{prop:<key_width$}{ANSI_RESET}: {}\n",
            wrapped.first().map(|s| s.as_str()).unwrap_or("")
        ));
    } else {
        out.push_str(&format!(
            "{indent_prefix}{color}{prop:<key_width$}{ANSI_RESET}: {}\n",
            wrapped[0]
        ));
        for line in &wrapped[1..] {
            out.push_str(&format!("{cont_indent}{line}\n"));
        }
    }
    out
}

fn format_section_header(title: &str) -> String {
    let pad = title.chars().count() + 4;
    let top = format!(
        "  {ANSI_BOLD}{ANSI_CYAN}┌{}┐{ANSI_RESET}\n",
        "─".repeat(pad)
    );
    let mid = format!(
        "  {ANSI_BOLD}{ANSI_CYAN}│{ANSI_RESET}  {ANSI_BOLD}{ANSI_BRIGHT_WHITE}{title}{ANSI_RESET}  {ANSI_BOLD}{ANSI_CYAN}│{ANSI_RESET}\n"
    );
    let bot = format!(
        "  {ANSI_BOLD}{ANSI_CYAN}└{}┘{ANSI_RESET}\n",
        "─".repeat(pad)
    );
    format!("{top}{mid}{bot}")
}
