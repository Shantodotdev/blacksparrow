//! One walk over the cleaned DOM that produces typed [`Block`]s (with heading paths and source
//! selectors), numbered link references, and GitHub-flavoured Markdown.

use crate::extract::clean::node_name;
use crate::extract::types::{Block, BlockKind, DocLink};
use dom_query::NodeRef;
use hashbrown::HashMap;
use url::Url;

/// Output of [`convert`].
#[derive(Debug, Default)]
pub struct Converted {
    /// Content blocks in document order.
    pub blocks: Vec<Block>,
    /// Links referenced from the Markdown, in reference-number order.
    pub links: Vec<DocLink>,
}

const BLOCK_TAGS: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "dd",
    "details",
    "dialog",
    "div",
    "dl",
    "dt",
    "fieldset",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hgroup",
    "hr",
    "li",
    "main",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "summary",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "tr",
    "ul",
    "body",
    "html",
    "center",
];

/// Phrasing tags that format part of a word; no separator is inserted around them.
const FORMATTING_TAGS: &[&str] = &[
    "a", "abbr", "b", "code", "em", "i", "kbd", "mark", "s", "small", "strong", "sub", "sup", "u",
    "del", "ins", "q", "var", "samp", "time",
];

fn is_block_tag(name: &str) -> bool {
    BLOCK_TAGS.contains(&name)
}

fn heading_level(name: &str) -> Option<u8> {
    match name {
        "h1" => Some(1),
        "h2" => Some(2),
        "h3" => Some(3),
        "h4" => Some(4),
        "h5" => Some(5),
        "h6" => Some(6),
        _ => None,
    }
}

/// Converts the subtrees under `roots` into blocks and links. `base` resolves relative URLs.
pub fn convert(roots: &[NodeRef], base: &Url) -> Converted {
    let mut walker = Walker {
        base,
        blocks: Vec::new(),
        links: Vec::new(),
        link_index: HashMap::new(),
        headings: Vec::new(),
        pending: Inline::default(),
        pending_node: None,
    };
    for root in roots {
        walker.walk(root);
        walker.flush();
    }
    Converted {
        blocks: walker.blocks,
        links: walker.links,
    }
}

/// Renders blocks plus link references as one Markdown document.
pub fn render_markdown(blocks: &[Block], links: &[DocLink]) -> String {
    let mut out = String::new();
    for block in blocks {
        if block.markdown.trim().is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(&block.markdown);
    }
    let used: Vec<(usize, &DocLink)> = links
        .iter()
        .enumerate()
        .map(|(i, l)| (i + 1, l))
        .filter(|(n, _)| out.contains(&format!("][{n}]")))
        .collect();
    if !used.is_empty() {
        out.push_str("\n\n");
        for (n, link) in used {
            out.push_str(&format!("[{n}]: {}\n", link.url));
        }
    } else if !out.is_empty() {
        out.push('\n');
    }
    out
}

/// Plain text of all blocks, one block per paragraph.
pub fn render_text(blocks: &[Block]) -> String {
    blocks
        .iter()
        .map(|b| b.text.as_str())
        .filter(|t| !t.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
}

#[derive(Default, Debug)]
struct Inline {
    md: String,
    text: String,
}

impl Inline {
    fn push_text(&mut self, raw: &str) {
        let collapsed = collapse_ws(raw);
        if collapsed.is_empty() {
            return;
        }
        let leading = raw.starts_with(char::is_whitespace);
        let trailing = raw.ends_with(char::is_whitespace);
        let core = collapsed.trim();
        if leading {
            self.space();
        }
        self.md.push_str(&escape_md(core));
        self.text.push_str(core);
        if trailing && !core.is_empty() {
            self.space();
        }
    }

    fn space(&mut self) {
        if !self.md.is_empty() && !self.md.ends_with([' ', '\n']) {
            self.md.push(' ');
        }
        if !self.text.is_empty() && !self.text.ends_with([' ', '\n']) {
            self.text.push(' ');
        }
    }

    fn newline(&mut self) {
        let md = self.md.trim_end().to_string();
        self.md = md;
        let text = self.text.trim_end().to_string();
        self.text = text;
        if !self.md.is_empty() {
            self.md.push('\n');
        }
        if !self.text.is_empty() {
            self.text.push('\n');
        }
    }

    fn append(&mut self, other: Inline) {
        self.md.push_str(&other.md);
        self.text.push_str(&other.text);
    }

    fn finish(self) -> (String, String) {
        (tidy(&self.md), tidy(&self.text))
    }

    fn is_blank(&self) -> bool {
        self.text.trim().is_empty() && !self.md.contains("![")
    }
}

fn collapse_ws(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_ws = false;
    for c in s.chars() {
        if c.is_whitespace() {
            if !in_ws {
                out.push(' ');
                in_ws = true;
            }
        } else {
            out.push(c);
            in_ws = false;
        }
    }
    out
}

fn tidy(s: &str) -> String {
    s.lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Escapes characters that would otherwise start Markdown syntax inside prose.
fn escape_md(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '*' | '_' | '`' | '[' | ']') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

struct Walker<'u, 'a> {
    base: &'u Url,
    blocks: Vec<Block>,
    links: Vec<DocLink>,
    link_index: HashMap<String, usize>,
    headings: Vec<(u8, String)>,
    pending: Inline,
    pending_node: Option<NodeRef<'a>>,
}

impl<'a> Walker<'_, 'a> {
    fn heading_path(&self) -> Vec<String> {
        self.headings.iter().map(|(_, t)| t.clone()).collect()
    }

    fn push_block(&mut self, kind: BlockKind, text: String, markdown: String, node: &NodeRef) {
        if text.trim().is_empty() && !matches!(kind, BlockKind::Image { .. }) {
            return;
        }
        self.blocks.push(Block {
            kind,
            text,
            heading_path: self.heading_path(),
            selector: css_path(node),
            markdown,
        });
    }

    /// Emits loose inline content collected between block elements as a paragraph.
    fn flush(&mut self) {
        let pending = std::mem::take(&mut self.pending);
        let node = self.pending_node.take();
        if pending.is_blank() {
            return;
        }
        let (md, text) = pending.finish();
        if let Some(node) = node {
            let text = if text.is_empty() { md.clone() } else { text };
            self.push_block(BlockKind::Paragraph, text, md, &node);
        }
    }

    fn walk(&mut self, node: &NodeRef<'a>) {
        if node.is_text() {
            let text = node.text();
            if !text.trim().is_empty() && self.pending_node.is_none() {
                self.pending_node = node.parent();
            }
            self.pending.push_text(&text);
            return;
        }
        if !node.is_element() && !node.is_document() {
            return;
        }
        let name = node_name(node);

        if let Some(level) = heading_level(&name) {
            self.flush();
            let inline = self.inline(node);
            let (md, text) = inline.finish();
            let text = text.replace('\n', " ");
            if text.is_empty() {
                return;
            }
            while self.headings.last().is_some_and(|(l, _)| *l >= level) {
                self.headings.pop();
            }
            let markdown = format!("{} {}", "#".repeat(level as usize), md.replace('\n', " "));
            self.push_block(BlockKind::Heading { level }, text.clone(), markdown, node);
            self.headings.push((level, text));
            return;
        }

        match name.as_str() {
            "p" => {
                self.flush();
                let (md, text) = self.inline(node).finish();
                self.push_block(BlockKind::Paragraph, text, md, node);
            }
            "ul" | "ol" | "menu" => {
                self.flush();
                self.list(node, name == "ol");
            }
            "dl" => {
                self.flush();
                self.definition_list(node);
            }
            "table" if !is_layout_table(node) => {
                self.flush();
                self.table(node);
            }
            "pre" => {
                self.flush();
                self.code(node);
            }
            "blockquote" => {
                self.flush();
                let (md, text) = self.inline_blocks(node).finish();
                let quoted = md
                    .lines()
                    .map(|l| format!("> {l}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                self.push_block(BlockKind::Quote, text, quoted, node);
            }
            "img" => {
                self.flush();
                if let Some((src, alt)) = self.image(node) {
                    let markdown = format!("![{}]({src})", escape_md(&alt));
                    self.blocks.push(Block {
                        kind: BlockKind::Image {
                            src,
                            alt: alt.clone(),
                        },
                        text: alt,
                        heading_path: self.heading_path(),
                        selector: css_path(node),
                        markdown,
                    });
                }
            }
            "br" => {
                self.pending.newline();
            }
            "hr" => self.flush(),
            _ if !is_block_tag(&name) && !has_block_descendant(node) => {
                // Inline element in block context: part of the surrounding paragraph.
                if self.pending_node.is_none() {
                    self.pending_node = node.parent();
                }
                let inline = self.inline(node);
                if separates(&name) {
                    self.pending.space();
                }
                self.pending.append(inline);
                if separates(&name) {
                    self.pending.space();
                }
            }
            _ if is_block_tag(&name)
                && !has_block_descendant(node)
                && name != "td"
                && !node.text().trim().is_empty() =>
            {
                // A leaf container (`<div>text</div>`) is a paragraph of its own.
                self.flush();
                let (md, text) = self.inline(node).finish();
                self.push_block(BlockKind::Paragraph, text, md, node);
            }
            _ => {
                let block = is_block_tag(&name);
                if block {
                    self.flush();
                }
                for child in node.children_it(false) {
                    self.walk(&child);
                }
                if block {
                    self.flush();
                }
            }
        }
    }

    /// Renders an element's content as inline Markdown and plain text.
    fn inline(&mut self, node: &NodeRef) -> Inline {
        let mut out = Inline::default();
        for child in node.children_it(false) {
            self.inline_into(&child, &mut out);
        }
        out
    }

    /// Like [`Self::inline`] but block children start new lines (for quotes and list items).
    fn inline_blocks(&mut self, node: &NodeRef) -> Inline {
        let mut out = Inline::default();
        for child in node.children_it(false) {
            let name = node_name(&child);
            if child.is_element() && is_block_tag(&name) {
                out.newline();
                self.inline_into(&child, &mut out);
                out.newline();
            } else {
                self.inline_into(&child, &mut out);
            }
        }
        out
    }

    fn inline_into(&mut self, node: &NodeRef, out: &mut Inline) {
        if node.is_text() {
            out.push_text(&node.text());
            return;
        }
        if !node.is_element() {
            return;
        }
        let name = node_name(node);
        match name.as_str() {
            "br" => out.newline(),
            "img" => {
                if let Some((src, alt)) = self.image(node) {
                    out.space();
                    out.md.push_str(&format!("![{}]({src})", escape_md(&alt)));
                    out.text.push_str(&alt);
                    out.space();
                }
            }
            "a" => {
                let inner = self.inline(node);
                let (md, text) = inner.finish();
                if text.is_empty() && !md.contains("![") {
                    return;
                }
                let href = node.attr("href").map(|h| h.to_string()).unwrap_or_default();
                match self.resolve(&href) {
                    Some(url) => {
                        let n = self.link_number(&url, &text);
                        out.md.push_str(&format!("[{md}][{n}]"));
                    }
                    None => out.md.push_str(&md),
                }
                out.text.push_str(&text);
            }
            "strong" | "b" => wrap(out, self.inline(node), "**"),
            "em" | "i" => wrap(out, self.inline(node), "*"),
            "del" | "s" | "strike" => wrap(out, self.inline(node), "~~"),
            "code" | "kbd" | "samp" => {
                let raw = collapse_ws(&node.text());
                let raw = raw.trim();
                if !raw.is_empty() {
                    let fence = if raw.contains('`') { "``" } else { "`" };
                    out.md.push_str(&format!("{fence}{raw}{fence}"));
                    out.text.push_str(raw);
                }
            }
            "ul" | "ol" => {
                out.newline();
                for (i, item) in node
                    .element_children()
                    .iter()
                    .filter(|c| node_name(c) == "li")
                    .enumerate()
                {
                    let (md, text) = self.inline(item).finish();
                    let marker = if name == "ol" {
                        format!("{}.", i + 1)
                    } else {
                        "-".to_string()
                    };
                    out.md
                        .push_str(&format!("  {marker} {}\n", md.replace('\n', " ")));
                    out.text.push_str(&format!("{text}\n"));
                }
            }
            _ => {
                let sep = separates(&name) || is_block_tag(&name);
                if sep {
                    out.space();
                }
                for child in node.children_it(false) {
                    self.inline_into(&child, out);
                }
                if sep {
                    out.space();
                }
            }
        }
    }

    fn list(&mut self, node: &NodeRef, ordered: bool) {
        let mut items = Vec::new();
        let mut md_lines = Vec::new();
        for item in node.element_children() {
            if node_name(&item) != "li" {
                continue;
            }
            let (md, text) = self.inline(&item).finish();
            if text.is_empty() && !md.contains("![") {
                continue;
            }
            let marker = if ordered {
                format!("{}.", items.len() + 1)
            } else {
                "-".to_string()
            };
            let mut lines = md.lines();
            let first = lines.next().unwrap_or_default();
            let mut entry = format!("{marker} {first}");
            for rest in lines {
                entry.push('\n');
                if rest.trim_start().starts_with(['-', '*']) || starts_numbered(rest.trim_start()) {
                    entry.push_str(&format!("  {}", rest.trim_start()));
                } else {
                    entry.push_str(&format!("  {rest}"));
                }
            }
            md_lines.push(entry);
            items.push(text.replace('\n', " "));
        }
        if items.is_empty() {
            return;
        }
        let text = items.join("\n");
        self.push_block(
            BlockKind::List { ordered, items },
            text,
            md_lines.join("\n"),
            node,
        );
    }

    fn definition_list(&mut self, node: &NodeRef) {
        let mut items = Vec::new();
        let mut md_lines = Vec::new();
        let mut term: Option<(String, String)> = None;
        for child in node.element_children() {
            match node_name(&child).as_str() {
                "dt" => {
                    if let Some((md, text)) = term.take() {
                        md_lines.push(format!("- {md}"));
                        items.push(text);
                    }
                    term = Some(self.inline(&child).finish());
                }
                "dd" => {
                    let (md, text) = self.inline(&child).finish();
                    match term.take() {
                        Some((tmd, ttext)) => {
                            md_lines.push(format!("- {tmd}: {md}"));
                            items.push(format!("{ttext}: {text}"));
                        }
                        None => {
                            md_lines.push(format!("- {md}"));
                            items.push(text);
                        }
                    }
                }
                _ => {}
            }
        }
        if let Some((md, text)) = term {
            md_lines.push(format!("- {md}"));
            items.push(text);
        }
        if items.is_empty() {
            return;
        }
        let text = items.join("\n");
        self.push_block(
            BlockKind::List {
                ordered: false,
                items,
            },
            text,
            md_lines.join("\n"),
            node,
        );
    }

    fn table(&mut self, node: &NodeRef) {
        let mut header: Vec<String> = Vec::new();
        let mut header_md: Vec<String> = Vec::new();
        let mut rows: Vec<Vec<String>> = Vec::new();
        let mut rows_md: Vec<Vec<String>> = Vec::new();

        let trs: Vec<NodeRef> = node
            .descendants_it()
            .filter(|d| node_name(d) == "tr" && nearest_table_is(d, node))
            .collect();
        for (index, tr) in trs.iter().enumerate() {
            let cells: Vec<NodeRef> = tr
                .element_children()
                .into_iter()
                .filter(|c| matches!(node_name(c).as_str(), "td" | "th"))
                .collect();
            if cells.is_empty() {
                continue;
            }
            let in_thead = tr.ancestors_it(None).any(|a| node_name(&a) == "thead");
            let all_th = cells.iter().all(|c| node_name(c) == "th");
            let mut texts = Vec::new();
            let mut mds = Vec::new();
            for cell in &cells {
                let (md, text) = self.inline(cell).finish();
                texts.push(text.replace('\n', " "));
                mds.push(md.replace('\n', " ").replace('|', "\\|"));
            }
            if header.is_empty() && rows.is_empty() && index == 0 && (in_thead || all_th) {
                header = texts;
                header_md = mds;
            } else {
                rows.push(texts);
                rows_md.push(mds);
            }
        }
        if header.is_empty() && rows.is_empty() {
            return;
        }
        let width = rows
            .iter()
            .map(Vec::len)
            .chain(std::iter::once(header.len()))
            .max()
            .unwrap_or(0);
        if width == 0 {
            return;
        }
        let pad = |cells: &[String]| {
            let mut v: Vec<String> = cells.to_vec();
            v.resize(width, String::new());
            v
        };
        let head_md = if header_md.is_empty() {
            vec![String::new(); width]
        } else {
            pad(&header_md)
        };
        let mut md = format!("| {} |\n", head_md.join(" | "));
        md.push_str(&format!("| {} |", vec!["---"; width].join(" | ")));
        for row in &rows_md {
            md.push_str(&format!("\n| {} |", pad(row).join(" | ")));
        }
        let mut text_lines = Vec::new();
        if !header.is_empty() {
            text_lines.push(header.join("\t"));
        }
        for row in &rows {
            text_lines.push(row.join("\t"));
        }
        self.push_block(
            BlockKind::Table { header, rows },
            text_lines.join("\n"),
            md,
            node,
        );
    }

    fn code(&mut self, node: &NodeRef) {
        let raw = node.text().to_string();
        let code = raw.trim_matches('\n').trim_end().to_string();
        if code.trim().is_empty() {
            return;
        }
        let language = code_language(node);
        let fence = if code.contains("```") { "````" } else { "```" };
        let markdown = format!(
            "{fence}{}\n{code}\n{fence}",
            language.as_deref().unwrap_or("")
        );
        self.push_block(BlockKind::Code { language }, code, markdown, node);
    }

    fn image(&self, node: &NodeRef) -> Option<(String, String)> {
        let src = node
            .attr("src")
            .or_else(|| node.attr("data-src"))
            .map(|s| s.to_string())
            .unwrap_or_default();
        let src = self.resolve(&src)?;
        let alt = collapse_ws(&node.attr("alt").map(|a| a.to_string()).unwrap_or_default())
            .trim()
            .to_string();
        Some((src, alt))
    }

    fn resolve(&self, href: &str) -> Option<String> {
        let href = href.trim();
        if href.is_empty() || href.starts_with('#') {
            return None;
        }
        let lower = href.to_ascii_lowercase();
        if lower.starts_with("javascript:") || lower.starts_with("data:") {
            return None;
        }
        self.base.join(href).ok().map(|u| u.to_string())
    }

    fn link_number(&mut self, url: &str, text: &str) -> usize {
        if let Some(&n) = self.link_index.get(url) {
            return n;
        }
        self.links.push(DocLink {
            url: url.to_string(),
            text: text.to_string(),
        });
        let n = self.links.len();
        self.link_index.insert(url.to_string(), n);
        n
    }
}

fn wrap(out: &mut Inline, inner: Inline, marker: &str) {
    let (md, text) = inner.finish();
    if text.is_empty() {
        out.md.push_str(&md);
        return;
    }
    out.md.push_str(&format!("{marker}{md}{marker}"));
    out.text.push_str(&text);
}

fn separates(name: &str) -> bool {
    !FORMATTING_TAGS.contains(&name)
}

fn starts_numbered(s: &str) -> bool {
    let digits = s.chars().take_while(char::is_ascii_digit).count();
    digits > 0 && s[digits..].starts_with(". ")
}

fn has_block_descendant(node: &NodeRef) -> bool {
    node.descendants_it().any(|d| {
        d.is_element() && {
            let n = node_name(&d);
            is_block_tag(&n)
        }
    })
}

fn nearest_table_is(tr: &NodeRef, table: &NodeRef) -> bool {
    tr.ancestors_it(None)
        .find(|a| node_name(a) == "table")
        .is_some_and(|t| t.id == table.id)
}

/// Tables used for page layout hold headings, nested tables or several paragraphs per cell;
/// those are walked as containers rather than converted to Markdown tables.
fn is_layout_table(node: &NodeRef) -> bool {
    node.descendants_it().any(|d| {
        let n = node_name(&d);
        n == "table" || heading_level(&n).is_some() || n == "pre" || n == "ul" || n == "ol"
    })
}

fn code_language(node: &NodeRef) -> Option<String> {
    let mut candidates = vec![*node];
    if let Some(code) = node
        .element_children()
        .into_iter()
        .find(|c| node_name(c) == "code")
    {
        candidates.push(code);
    }
    for candidate in candidates {
        if let Some(lang) = candidate
            .attr("data-lang")
            .or_else(|| candidate.attr("data-language"))
        {
            let lang = lang.trim().to_ascii_lowercase();
            if !lang.is_empty() {
                return Some(lang);
            }
        }
        let class = candidate
            .attr("class")
            .map(|c| c.to_string())
            .unwrap_or_default();
        for token in class.split_ascii_whitespace() {
            for prefix in ["language-", "lang-", "highlight-source-", "brush:"] {
                if let Some(lang) = token.strip_prefix(prefix) {
                    if !lang.is_empty() {
                        return Some(lang.to_ascii_lowercase());
                    }
                }
            }
        }
    }
    None
}

/// A short, reasonably stable CSS path for an element: the nearest ancestor `#id` (when the id
/// looks hand-written) followed by `tag:nth-of-type(n)` steps.
pub fn css_path(node: &NodeRef) -> String {
    let mut steps: Vec<String> = Vec::new();
    let mut current = Some(*node);
    while let Some(n) = current {
        if !n.is_element() {
            current = n.parent();
            continue;
        }
        let name = node_name(&n);
        if matches!(name.as_str(), "html" | "body") {
            break;
        }
        if let Some(id) = n.attr("id") {
            let id = id.trim();
            if is_stable_id(id) {
                steps.push(format!("#{}", css_escape(id)));
                break;
            }
        }
        let same: Vec<NodeRef> = n
            .parent()
            .map(|p| {
                p.element_children()
                    .into_iter()
                    .filter(|c| node_name(c) == name)
                    .collect()
            })
            .unwrap_or_default();
        if same.len() > 1 {
            let index = same.iter().position(|c| c.id == n.id).unwrap_or(0) + 1;
            steps.push(format!("{name}:nth-of-type({index})"));
        } else {
            steps.push(name);
        }
        current = n.parent();
    }
    steps.reverse();
    if steps.is_empty() {
        "body".to_string()
    } else {
        steps.join(" > ")
    }
}

fn is_stable_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 40
        && id.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        && id.chars().filter(char::is_ascii_digit).count() <= 3
}

fn css_escape(id: &str) -> String {
    id.chars()
        .flat_map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                vec![c]
            } else {
                vec!['\\', c]
            }
        })
        .collect()
}
