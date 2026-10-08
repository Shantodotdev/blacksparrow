//! PDF text extraction into Markdown with page markers (pure Rust, `lopdf`).

use crate::extract::document::finalize;
use crate::extract::types::{Block, BlockKind, PageDocument, PageStatus, ScrapeOptions};

/// Pages read from one PDF at most.
pub const MAX_PDF_PAGES: usize = 200;

/// Extracts the text of a PDF. Each page becomes a `## Page N` section. PDFs without any text
/// layer (scans) come back as [`PageStatus::NeedsOcr`] instead of guessed content.
pub fn pdf_to_document(bytes: &[u8], url: &str, opts: &ScrapeOptions) -> PageDocument {
    let mut doc = PageDocument::with_status(url, PageStatus::Ok);
    doc.source = "pdf".to_string();
    doc.content_type = "application/pdf".to_string();
    doc.extractor = "pdf".to_string();

    let pdf = match lopdf::Document::load_mem(bytes) {
        Ok(pdf) => pdf,
        Err(e) => {
            doc.status = PageStatus::Error;
            doc.error = Some(format!("Unreadable PDF: {e}"));
            return doc;
        }
    };
    if pdf.is_encrypted() {
        doc.status = PageStatus::Error;
        doc.error = Some("Encrypted PDF".to_string());
        return doc;
    }

    let pages: Vec<u32> = pdf.get_pages().keys().copied().collect();
    let total = pages.len();
    let mut blocks = Vec::new();
    let mut any_text = false;
    for number in pages.into_iter().take(MAX_PDF_PAGES) {
        let text = pdf.extract_text(&[number]).unwrap_or_default();
        let heading = format!("Page {number}");
        blocks.push(Block {
            kind: BlockKind::Heading { level: 2 },
            text: heading.clone(),
            heading_path: Vec::new(),
            selector: format!("page:{number}"),
            markdown: format!("## {heading}"),
        });
        for (i, paragraph) in split_paragraphs(&text).into_iter().enumerate() {
            any_text = true;
            blocks.push(Block {
                kind: BlockKind::Paragraph,
                text: paragraph.clone(),
                heading_path: vec![heading.clone()],
                selector: format!("page:{number}/p:{}", i + 1),
                markdown: paragraph,
            });
        }
    }

    if !any_text {
        doc.status = PageStatus::NeedsOcr;
        doc.error = Some(format!(
            "No text layer in {total} page(s); the PDF looks scanned and needs OCR"
        ));
        return doc;
    }
    doc.confidence = 1.0;
    doc.blocks = blocks;
    if total > MAX_PDF_PAGES {
        doc.truncated = true;
    }
    let truncated = doc.truncated;
    finalize(&mut doc, opts);
    doc.truncated |= truncated;
    doc
}

fn split_paragraphs(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            continue;
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(line);
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}
