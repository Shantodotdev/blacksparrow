//! PDFs: text layers become Markdown with page sections; scans and garbage are classified.

use blacksparrow::extract::pdf::pdf_to_document;
use blacksparrow::extract::scrape::{Scraper, ScraperConfig};
use blacksparrow::extract::{PageStatus, ScrapeOptions};
use lopdf::content::{Content, Operation};
use lopdf::{dictionary, Document, Object, Stream};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

/// Builds a PDF with one page per entry; each page shows its lines of text (an empty list
/// makes a page with no text layer, like a scan).
fn make_pdf(pages: &[&[&str]]) -> Vec<u8> {
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let font_id = doc.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
    });
    let resources_id = doc.add_object(dictionary! {
        "Font" => dictionary! { "F1" => font_id },
    });
    let mut kids = Vec::new();
    for lines in pages {
        let mut ops = Vec::new();
        if !lines.is_empty() {
            ops.push(Operation::new("BT", vec![]));
            ops.push(Operation::new("Tf", vec!["F1".into(), 12.into()]));
            ops.push(Operation::new("TL", vec![16.into()]));
            ops.push(Operation::new("Td", vec![72.into(), 720.into()]));
            for line in lines.iter() {
                ops.push(Operation::new("Tj", vec![Object::string_literal(*line)]));
                ops.push(Operation::new("T*", vec![]));
            }
            ops.push(Operation::new("ET", vec![]));
        } else {
            ops.push(Operation::new(
                "re",
                vec![0.into(), 0.into(), 10.into(), 10.into()],
            ));
            ops.push(Operation::new("f", vec![]));
        }
        let content = Content { operations: ops };
        let content_id = doc.add_object(Stream::new(dictionary! {}, content.encode().unwrap()));
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page",
            "Parent" => pages_id,
            "Contents" => content_id,
            "Resources" => resources_id,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        });
        kids.push(Object::from(page_id));
    }
    let count = kids.len() as i64;
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => kids,
            "Count" => count,
        }),
    );
    let catalog_id = doc.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    doc.trailer.set("Root", catalog_id);
    let mut bytes = Vec::new();
    doc.save_to(&mut bytes).unwrap();
    bytes
}

#[test]
fn text_pdfs_become_markdown_with_page_sections() {
    let pdf = make_pdf(&[
        &["Annual report 2025", "Revenue grew by twelve percent."],
        &["Outlook", "We expect steady growth next year."],
    ]);
    let doc = pdf_to_document(
        &pdf,
        "https://example.com/report.pdf",
        &ScrapeOptions::default(),
    );
    assert_eq!(doc.status, PageStatus::Ok, "{:?}", doc.error);
    assert_eq!(doc.source, "pdf");
    assert!(doc.markdown.contains("## Page 1"), "{}", doc.markdown);
    assert!(doc.markdown.contains("## Page 2"));
    assert!(doc.markdown.contains("Revenue grew by twelve percent."));
    assert!(doc.markdown.contains("We expect steady growth next year."));
    let page_two = doc.markdown.find("## Page 2").unwrap();
    assert!(doc.markdown.find("Outlook").unwrap() > page_two);
}

#[test]
fn scanned_pdfs_are_reported_as_needing_ocr() {
    let pdf = make_pdf(&[&[], &[]]);
    let doc = pdf_to_document(
        &pdf,
        "https://example.com/scan.pdf",
        &ScrapeOptions::default(),
    );
    assert_eq!(doc.status, PageStatus::NeedsOcr);
    assert!(doc.markdown.is_empty());
    assert!(doc.error.unwrap().contains("OCR"));
}

#[test]
fn garbage_is_an_error_not_content() {
    let doc = pdf_to_document(
        b"%PDF-1.4 this is not really a pdf",
        "https://example.com/broken.pdf",
        &ScrapeOptions::default(),
    );
    assert_eq!(doc.status, PageStatus::Error);
    assert!(doc.markdown.is_empty());
}

#[tokio::test]
async fn the_scraper_reads_pdf_responses() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/robots.txt"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/files/guide.pdf"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            make_pdf(&[&["Setup guide", "Plug in the cable."]]),
            "application/pdf",
        ))
        .mount(&server)
        .await;
    let scraper = Scraper::new(ScraperConfig {
        allowed_private_hosts: vec![server.address().to_string()],
        ..Default::default()
    })
    .unwrap();
    let doc = scraper
        .scrape(
            &format!("{}/files/guide.pdf", server.uri()),
            &ScrapeOptions::default(),
        )
        .await
        .unwrap();
    assert_eq!(doc.status, PageStatus::Ok, "{:?}", doc.error);
    assert_eq!(doc.source, "pdf");
    assert!(
        doc.markdown.contains("Plug in the cable."),
        "{}",
        doc.markdown
    );
}
