//! Markdown helpers: reading Markdown served by sites (the `Accept: text/markdown` fast path)
//! into blocks, and writing documents as Markdown files with YAML frontmatter.

use crate::extract::types::{Block, BlockKind, PageDocument};

/// Splits Markdown into typed blocks with heading paths. Handles ATX headings, fenced code,
/// lists, pipe tables, blockquotes and paragraphs; anything else is kept as paragraph text.
pub fn markdown_to_blocks(markdown: &str) -> Vec<Block> {
    let lines: Vec<&str> = markdown.lines().collect();
    let mut blocks = Vec::new();
    let mut headings: Vec<(u8, String)> = Vec::new();
    let mut i = 0usize;

    let path = |headings: &Vec<(u8, String)>| -> Vec<String> {
        headings.iter().map(|(_, t)| t.clone()).collect()
    };

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            i += 1;
            continue;
        }

        if let Some((level, text)) = atx_heading(trimmed) {
            while headings.last().is_some_and(|(l, _)| *l >= level) {
                headings.pop();
            }
            blocks.push(Block {
                kind: BlockKind::Heading { level },
                text: strip_inline(&text),
                heading_path: path(&headings),
                selector: format!("line:{}", i + 1),
                markdown: format!("{} {}", "#".repeat(level as usize), text),
            });
            headings.push((level, strip_inline(&text)));
            i += 1;
            continue;
        }

        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            let fence: String = trimmed
                .chars()
                .take_while(|c| *c == '`' || *c == '~')
                .collect();
            let language = trimmed[fence.len()..].trim();
            let start = i;
            let mut body = Vec::new();
            i += 1;
            while i < lines.len() && !lines[i].trim_start().starts_with(&fence) {
                body.push(lines[i]);
                i += 1;
            }
            let end = i.min(lines.len().saturating_sub(1));
            i += 1;
            blocks.push(Block {
                kind: BlockKind::Code {
                    language: (!language.is_empty()).then(|| language.to_string()),
                },
                text: body.join("\n"),
                heading_path: path(&headings),
                selector: format!("line:{}", start + 1),
                markdown: lines[start..=end].join("\n"),
            });
            continue;
        }

        let start = i;
        let kind_of = |l: &str| -> u8 {
            let t = l.trim_start();
            if t.starts_with('|') {
                1
            } else if t.starts_with('>') {
                2
            } else if list_marker(t).is_some() {
                3
            } else {
                0
            }
        };
        let kind = kind_of(trimmed);
        let mut group = vec![line];
        i += 1;
        while i < lines.len() {
            let next = lines[i];
            if next.trim().is_empty() || atx_heading(next.trim_start()).is_some() {
                break;
            }
            let next_kind = kind_of(next);
            let continues = match kind {
                3 => next_kind == 3 || next.starts_with("  ") || next.starts_with('\t'),
                k => next_kind == k,
            };
            if !continues {
                break;
            }
            group.push(next);
            i += 1;
        }
        let markdown = group.join("\n");
        let selector = format!("line:{}", start + 1);
        let block = match kind {
            1 => {
                let mut rows: Vec<Vec<String>> = group
                    .iter()
                    .filter(|l| !is_separator_row(l))
                    .map(|l| split_row(l))
                    .collect();
                let header = if group.len() > 1 && is_separator_row(group[1]) && !rows.is_empty() {
                    rows.remove(0)
                } else {
                    Vec::new()
                };
                let mut text_lines = Vec::new();
                if !header.is_empty() {
                    text_lines.push(header.join("\t"));
                }
                text_lines.extend(rows.iter().map(|r| r.join("\t")));
                Block {
                    kind: BlockKind::Table { header, rows },
                    text: text_lines.join("\n"),
                    heading_path: path(&headings),
                    selector,
                    markdown,
                }
            }
            2 => {
                let text = group
                    .iter()
                    .map(|l| l.trim_start().trim_start_matches('>').trim())
                    .collect::<Vec<_>>()
                    .join(" ");
                Block {
                    kind: BlockKind::Quote,
                    text: strip_inline(&text),
                    heading_path: path(&headings),
                    selector,
                    markdown,
                }
            }
            3 => {
                let ordered = list_marker(trimmed).is_some_and(|(o, _)| o);
                let items: Vec<String> = group
                    .iter()
                    .filter_map(|l| list_marker(l.trim_start()).map(|(_, rest)| strip_inline(rest)))
                    .collect();
                Block {
                    kind: BlockKind::List {
                        ordered,
                        items: items.clone(),
                    },
                    text: items.join("\n"),
                    heading_path: path(&headings),
                    selector,
                    markdown,
                }
            }
            _ => Block {
                kind: BlockKind::Paragraph,
                text: strip_inline(&group.join(" ")),
                heading_path: path(&headings),
                selector,
                markdown,
            },
        };
        blocks.push(block);
    }
    blocks
}

fn atx_heading(line: &str) -> Option<(u8, String)> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&hashes) && line[hashes..].starts_with(' ') {
        let text = line[hashes..]
            .trim()
            .trim_end_matches('#')
            .trim()
            .to_string();
        if !text.is_empty() {
            return Some((hashes as u8, text));
        }
    }
    None
}

fn list_marker(line: &str) -> Option<(bool, &str)> {
    for marker in ["- ", "* ", "+ "] {
        if let Some(rest) = line.strip_prefix(marker) {
            return Some((false, rest));
        }
    }
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if digits > 0 {
        let rest = &line[digits..];
        if let Some(rest) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            return Some((true, rest));
        }
    }
    None
}

fn is_separator_row(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('|') && t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
}

fn split_row(line: &str) -> Vec<String> {
    line.trim()
        .trim_start_matches('|')
        .trim_end_matches('|')
        .split('|')
        .map(|c| strip_inline(c.trim()))
        .collect()
}

/// Removes common inline Markdown syntax, keeping the visible text.
pub fn strip_inline(md: &str) -> String {
    let mut out = String::with_capacity(md.len());
    let chars: Vec<char> = md.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '\\' if i + 1 < chars.len() => {
                out.push(chars[i + 1]);
                i += 2;
            }
            '*' | '_' | '`' | '~' => i += 1,
            '!' if chars.get(i + 1) == Some(&'[') => i += 1,
            '[' => i += 1,
            ']' => {
                // Skip a following (url) or [ref].
                i += 1;
                if let Some(&open) = chars.get(i) {
                    let close = match open {
                        '(' => Some(')'),
                        '[' => Some(']'),
                        _ => None,
                    };
                    if let Some(close) = close {
                        if let Some(end) = chars[i..].iter().position(|&ch| ch == close) {
                            i += end + 1;
                        }
                    }
                }
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Renders a document as a Markdown file with a small YAML frontmatter header.
pub fn to_markdown_file(doc: &PageDocument) -> String {
    let mut out = String::from("---\n");
    out.push_str(&format!("url: {}\n", yaml_str(&doc.final_url)));
    if let Some(title) = &doc.metadata.title {
        out.push_str(&format!("title: {}\n", yaml_str(title)));
    }
    if let Some(description) = &doc.metadata.description {
        out.push_str(&format!("description: {}\n", yaml_str(description)));
    }
    if let Some(author) = &doc.metadata.author {
        out.push_str(&format!("author: {}\n", yaml_str(author)));
    }
    if let Some(published) = &doc.metadata.published {
        out.push_str(&format!("published: {}\n", yaml_str(published)));
    }
    if let Some(language) = &doc.metadata.language {
        out.push_str(&format!("language: {}\n", yaml_str(language)));
    }
    out.push_str(&format!("status: {}\n", doc.status.as_str()));
    out.push_str(&format!("tokens: {}\n", doc.tokens));
    out.push_str(&format!("fetched_at: {}\n", doc.fetched_at));
    out.push_str("---\n\n");
    out.push_str(&doc.markdown);
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn yaml_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}
