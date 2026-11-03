//! # Site Extraction Integration Tests
//!
//! Serves realistic small-business sites from `tests/fixtures/sites/` with wiremock and runs
//! `blacksparrow::extract::extract_site` against them: a static Bangla/English bakery, a
//! JavaScript-only SPA (rendered in Chromium when `BLACKSPARROW_TEST_CHROME` names an
//! executable), bot-challenge pages, the address guard, hostile input, and the CLI end to end.

use blacksparrow::extract::render::RenderOptions;
use blacksparrow::extract::{extract_site, ExtractOptions, Outcome, RenderMode, SiteExtract};
use std::path::{Path, PathBuf};
use std::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sites")
}

fn content_type(file: &str) -> &'static str {
    match file.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "css" => "text/css",
        "js" => "application/javascript",
        _ => "application/octet-stream",
    }
}

/// Mounts every file under `tests/fixtures/sites/<site>` at its relative path; `index.html`
/// is also served at `/`.
async fn serve_site(server: &MockServer, site: &str) {
    let root = fixtures().join(site);
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let file = entry.unwrap().path();
            if file.is_dir() {
                stack.push(file);
                continue;
            }
            let relative = file
                .strip_prefix(&root)
                .unwrap()
                .to_str()
                .unwrap()
                .to_string();
            let body = std::fs::read(&file).unwrap();
            let template = ResponseTemplate::new(200)
                .set_body_bytes(body)
                .insert_header("content-type", content_type(&relative));
            let mut mounts = vec![format!("/{relative}")];
            if relative == "index.html" {
                mounts.push("/".to_string());
            }
            for url_path in mounts {
                Mock::given(method("GET"))
                    .and(path(url_path))
                    .respond_with(template.clone())
                    .mount(server)
                    .await;
            }
        }
    }
}

async fn mount_html(server: &MockServer, url_path: &str, status: u16, body: impl Into<Vec<u8>>) {
    Mock::given(method("GET"))
        .and(path(url_path))
        .respond_with(
            ResponseTemplate::new(status)
                .set_body_bytes(body.into())
                .insert_header("content-type", "text/html; charset=utf-8"),
        )
        .mount(server)
        .await;
}

fn options(render: RenderMode) -> ExtractOptions {
    let mut options = ExtractOptions::new();
    options.allow_private_network = true;
    options.render = render;
    options.timeout = Duration::from_secs(60);
    options.request_timeout = Duration::from_secs(10);
    options
}

/// Options for tests that need Chromium; `None` (after a notice) when no executable is given.
fn chrome_options(render: RenderMode) -> Option<ExtractOptions> {
    let Ok(chrome) = std::env::var("BLACKSPARROW_TEST_CHROME") else {
        eprintln!("skipped: BLACKSPARROW_TEST_CHROME is not set");
        return None;
    };
    let mut options = options(render);
    options.render_options = RenderOptions {
        chrome_path: Some(PathBuf::from(chrome)),
        no_sandbox: true,
        page_timeout: Duration::from_secs(25),
        ..RenderOptions::default()
    };
    Some(options)
}

async fn run(server: &MockServer, url_path: &str, options: ExtractOptions) -> SiteExtract {
    extract_site(&format!("{}{url_path}", server.uri()), options)
        .await
        .unwrap()
}

// ---------------------------------------------------------------------------
// Static bakery site
// ---------------------------------------------------------------------------

#[tokio::test]
async fn bakery_site_is_read_completely_without_rendering() {
    let server = MockServer::start().await;
    serve_site(&server, "bakery").await;
    // Anything the extractor should not request fails the test through a 404 assertion below.
    let result = run(&server, "/", options(RenderMode::Auto)).await;

    assert_eq!(result.schema_version, 1);
    assert_eq!(result.outcome, Outcome::Ok);
    assert!(result.pages.iter().all(|p| !p.rendered));

    // Pages: home, then contact, about, catalog by rule; cart and privacy never.
    let kinds: Vec<(&str, String)> = result
        .pages
        .iter()
        .map(|p| (p.kind, p.url.trim_start_matches(&server.uri()).to_string()))
        .collect();
    assert_eq!(
        kinds,
        vec![
            ("home", "/".to_string()),
            ("contact", "/contact.html".to_string()),
            ("about", "/about.html".to_string()),
            ("catalog", "/shop.html".to_string()),
        ]
    );
    assert!(result.page_errors.is_empty());
    assert_eq!(result.pages[0].lang.as_deref(), Some("bn"));
    assert!(result.pages[0]
        .title
        .as_deref()
        .unwrap()
        .contains("Sweet Crumbs"));
    assert!(result.pages[1].markdown.contains("Dhanmondi"));

    // Contacts: exact values, deduplicated across pages.
    assert_eq!(result.contacts.emails, vec!["hello@sweetcrumbs.com.bd"]);
    assert!(
        result
            .contacts
            .phones
            .iter()
            .any(|p| p.contains("01711") || p.contains("০১৭১১")),
        "phones: {:?}",
        result.contacts.phones
    );
    assert!(
        result.contacts.phones.len() <= 4,
        "{:?}",
        result.contacts.phones
    );
    assert_eq!(
        result.contacts.whatsapp,
        vec!["https://wa.me/8801711223344"]
    );
    assert_eq!(
        result.contacts.facebook,
        vec!["https://www.facebook.com/SweetCrumbsBD"]
    );
    assert_eq!(
        result.contacts.instagram,
        vec!["https://www.instagram.com/sweetcrumbs.bd/"]
    );

    // Structured data.
    assert_eq!(result.structured.names, vec!["Sweet Crumbs Bakery"]);
    assert!(result
        .structured
        .addresses
        .iter()
        .any(|a| a.contains("Dhanmondi")));
    assert!(result
        .structured
        .telephones
        .contains(&"+8801711223344".to_string()));

    // Design.
    let design = result.design.as_ref().unwrap();
    assert!(design
        .theme_color
        .as_deref()
        .is_some_and(|c| c.eq_ignore_ascii_case("#c0392b")));
    assert_eq!(design.site_name.as_deref(), Some("Sweet Crumbs"));
    let brand = design
        .colors
        .iter()
        .find(|c| c.hex.eq_ignore_ascii_case("#c0392b"))
        .expect("brand colour");
    let kinds: Vec<&str> = brand.evidence.iter().map(|e| e.kind).collect();
    assert!(kinds.contains(&"theme_color_meta"), "{kinds:?}");
    assert!(brand.occurrences >= 2);
    assert!(design
        .colors
        .iter()
        .any(|c| c.hex.eq_ignore_ascii_case("#2e86c1")));
    assert!(design
        .colors
        .iter()
        .any(|c| c.hex.eq_ignore_ascii_case("#222222")));
    assert!(design
        .colors
        .windows(2)
        .all(|w| w[0].occurrences >= w[1].occurrences));

    let playfair = design
        .fonts
        .iter()
        .find(|f| f.family == "Playfair Display")
        .unwrap();
    assert!(playfair.selectors.contains(&"headings"), "{playfair:?}");
    assert!(playfair.weights.contains(&400) && playfair.weights.contains(&700));
    let hind = design
        .fonts
        .iter()
        .find(|f| f.family == "Hind Siliguri")
        .unwrap();
    assert!(hind.selectors.contains(&"body"), "{hind:?}");
    assert!(hind.weights.contains(&500));

    // Logos: header logo image, then inline SVG, schema/other candidates after.
    let first = &design.logos[0];
    assert!(
        first.url.as_deref().unwrap().ends_with("/img/logo.png"),
        "{first:?}"
    );
    assert!(design
        .logos
        .iter()
        .any(|l| l.svg.as_deref().is_some_and(|s| s.contains("<svg"))));
    let reasons: Vec<_> = design.logos.iter().map(|l| l.reason).collect();
    assert!(reasons.windows(2).all(|w| w[0] <= w[1]), "{reasons:?}");

    // Stylesheets: main.css plus its @import; Google Fonts is not fetched.
    let sheets: Vec<&str> = design.stylesheets.iter().map(|s| s.url.as_str()).collect();
    assert_eq!(sheets.len(), 2, "{sheets:?}");
    assert!(sheets[0].ends_with("/css/main.css"));
    assert!(sheets[1].ends_with("/css/theme.css"));
    assert!(design
        .stylesheets
        .iter()
        .all(|s| s.status == Some(200) && s.error.is_none()));

    // Stats: 4 pages + 2 stylesheets, no browser.
    assert_eq!(result.stats.http_fetches, 6);
    assert_eq!(result.stats.rendered_pages, 0);
    assert!(result.stats.http_bytes > 0);
}

#[tokio::test]
async fn bakery_cart_and_privacy_are_never_fetched() {
    let server = MockServer::start().await;
    serve_site(&server, "bakery").await;
    let _ = run(&server, "/", options(RenderMode::Never)).await;
    let requested: Vec<String> = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| r.url.path().to_string())
        .collect();
    assert!(
        !requested.iter().any(|p| p == "/cart" || p == "/privacy"),
        "{requested:?}"
    );
}

#[tokio::test]
async fn caller_chosen_pages_replace_rule_selection() {
    let server = MockServer::start().await;
    serve_site(&server, "bakery").await;
    let mut opts = options(RenderMode::Never);
    opts.pages = vec![
        format!("{}/shop.html", server.uri()),
        format!("{}/missing.html", server.uri()),
    ];
    let result = run(&server, "/", opts).await;
    assert_eq!(result.outcome, Outcome::Ok);
    assert_eq!(result.pages.len(), 2);
    assert_eq!(result.pages[1].kind, "requested");
    assert_eq!(result.page_errors.len(), 1);
    assert!(result.page_errors[0].url.ends_with("/missing.html"));
}

// ---------------------------------------------------------------------------
// JavaScript SPA
// ---------------------------------------------------------------------------

#[tokio::test]
async fn spa_without_rendering_is_nearly_empty() {
    let server = MockServer::start().await;
    serve_site(&server, "spa").await;
    let result = run(&server, "/", options(RenderMode::Never)).await;
    assert_eq!(result.outcome, Outcome::Ok);
    let home = &result.pages[0];
    assert!(!home.rendered);
    assert!(home.word_count < 20, "words: {}", home.word_count);
    assert!(home.phones.is_empty() && home.emails.is_empty());
    assert_eq!(result.pages.len(), 1);
    assert_eq!(result.stats.rendered_pages, 0);
}

#[tokio::test]
async fn spa_is_rendered_in_chromium_when_auto() {
    let Some(opts) = chrome_options(RenderMode::Auto) else {
        return;
    };
    let server = MockServer::start().await;
    serve_site(&server, "spa").await;
    let started = std::time::Instant::now();
    let result = run(&server, "/", opts).await;
    eprintln!("spa render took {:?}", started.elapsed());

    assert_eq!(result.outcome, Outcome::Ok);
    let home = &result.pages[0];
    assert!(home.rendered);
    assert_eq!(home.render_reason, Some("js_shell"));
    assert!(home.markdown.contains("Bespoke suits"));
    assert_eq!(home.emails, vec!["orders@dhakatailors.test"]);
    assert!(
        home.phones.iter().any(|p| p.contains("01811")),
        "{:?}",
        home.phones
    );
    assert!(result.stats.rendered_pages >= 1);

    // The CSS-in-JS rule exists only in the CSSOM.
    let design = result.design.as_ref().unwrap();
    let purple = design
        .colors
        .iter()
        .find(|c| c.hex.eq_ignore_ascii_case("#8e44ad"))
        .expect("CSSOM colour");
    assert!(!purple.evidence.is_empty());
    assert!(design.fonts.iter().any(|f| f.family == "Lora"));

    // The link the script created leads to the contact page, read as a second page.
    assert!(
        result.pages.iter().any(|p| p.kind == "contact"),
        "{:?}",
        result.pages.iter().map(|p| p.kind).collect::<Vec<_>>()
    );
}

/// The page asks for an image and for the cloud metadata address. Images are always blocked in
/// the browser; the metadata address is only blocked when private networks are refused, and
/// the test server itself lives on loopback, so that half is covered by the guard tests and
/// the unit tests of `guard.rs` instead of by weakening the policy here.
#[tokio::test]
async fn rendered_page_requests_for_images_are_blocked() {
    let Some(opts) = chrome_options(RenderMode::Always) else {
        return;
    };
    let server = MockServer::start().await;
    serve_site(&server, "spa").await;
    Mock::given(method("GET"))
        .and(path("/pixel.png"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(vec![0u8; 8])
                .insert_header("content-type", "image/png"),
        )
        .mount(&server)
        .await;
    let result = run(&server, "/", opts).await;
    assert!(
        result.stats.blocked_browser_requests > 0,
        "{:?}",
        result.stats
    );
    let requested: Vec<String> = server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| r.url.path().to_string())
        .collect();
    assert!(
        !requested.iter().any(|p| p == "/pixel.png"),
        "{requested:?}"
    );
}

// ---------------------------------------------------------------------------
// Bot challenges
// ---------------------------------------------------------------------------

#[tokio::test]
async fn challenge_page_is_bot_protected() {
    let server = MockServer::start().await;
    let body = std::fs::read(fixtures().join("hostile/challenge.html")).unwrap();
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_bytes(body)
                .insert_header("content-type", "text/html; charset=utf-8")
                .insert_header("cf-mitigated", "challenge")
                .insert_header("server", "cloudflare"),
        )
        .mount(&server)
        .await;
    let result = run(&server, "/", options(RenderMode::Never)).await;
    assert_eq!(result.outcome, Outcome::BotProtected);
    assert!(result.outcome_detail.is_some());
    assert_eq!(result.pages[0].status, 403);
}

#[tokio::test]
async fn real_content_embedding_challenge_script_is_ok() {
    let server = MockServer::start().await;
    let body = std::fs::read(fixtures().join("hostile/protected_but_real.html")).unwrap();
    mount_html(&server, "/", 200, body).await;
    let result = run(&server, "/", options(RenderMode::Never)).await;
    assert_eq!(result.outcome, Outcome::Ok);
    assert!(result.pages[0].waf.is_none());
    assert!(result.pages[0].word_count > 150);
}

// ---------------------------------------------------------------------------
// Address guard and failures
// ---------------------------------------------------------------------------

#[tokio::test]
async fn private_addresses_are_refused_by_default() {
    let server = MockServer::start().await;
    serve_site(&server, "bakery").await;
    let mut opts = options(RenderMode::Never);
    opts.allow_private_network = false;
    let result = run(&server, "/", opts).await;
    assert_eq!(result.outcome, Outcome::BlockedAddress);
    assert!(result.pages.is_empty() && result.design.is_none());
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn link_local_metadata_address_is_refused_by_default() {
    let mut strict = options(RenderMode::Never);
    strict.allow_private_network = false;
    let result = extract_site("http://169.254.169.254/latest/meta-data", strict)
        .await
        .unwrap();
    assert_eq!(result.outcome, Outcome::BlockedAddress);
}

#[tokio::test]
async fn redirect_to_non_http_scheme_is_not_followed() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", "file:///etc/passwd"))
        .mount(&server)
        .await;
    let result = run(&server, "/", options(RenderMode::Never)).await;
    assert_ne!(result.outcome, Outcome::Ok);
    assert!(result.pages.is_empty());
}

#[tokio::test]
async fn redirect_to_refused_address_is_blocked() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", "http://169.254.169.254/latest/meta-data"),
        )
        .mount(&server)
        .await;
    let result = run(&server, "/", options(RenderMode::Never)).await;
    // Loopback is allowed here, so the metadata hop must fail one way or another.
    assert_ne!(result.outcome, Outcome::Ok, "{:?}", result.outcome_detail);
}

#[tokio::test]
async fn same_site_redirect_is_followed() {
    let server = MockServer::start().await;
    serve_site(&server, "bakery").await;
    Mock::given(method("GET"))
        .and(path("/old"))
        .respond_with(ResponseTemplate::new(301).insert_header("location", "/"))
        .mount(&server)
        .await;
    let result = run(&server, "/old", options(RenderMode::Never)).await;
    assert_eq!(result.outcome, Outcome::Ok);
    assert!(result.final_url.unwrap().ends_with('/'));
}

#[tokio::test]
async fn unresolvable_host_is_unresolved() {
    let result = extract_site("http://does-not-exist.invalid/", options(RenderMode::Never))
        .await
        .unwrap();
    assert_eq!(result.outcome, Outcome::Unresolved);
}

#[tokio::test]
async fn malformed_urls_are_unsupported() {
    for url in ["not a url", "ftp://example.com/", "file:///etc/passwd", ""] {
        let result = extract_site(url, options(RenderMode::Never)).await.unwrap();
        assert_eq!(result.outcome, Outcome::UnsupportedUrl, "{url}");
    }
}

// ---------------------------------------------------------------------------
// Hostile input
// ---------------------------------------------------------------------------

#[tokio::test]
async fn oversized_body_is_truncated_at_the_limit() {
    let server = MockServer::start().await;
    let filler = "<p>word word word word word word word word</p>\n";
    let mut body = String::from("<!doctype html><html><head><title>Big</title></head><body>");
    while body.len() < 5 * 1024 * 1024 {
        body.push_str(filler);
    }
    body.push_str("</body></html>");
    mount_html(&server, "/", 200, body).await;
    let mut opts = options(RenderMode::Never);
    opts.max_body_bytes = 256 * 1024;
    let result = run(&server, "/", opts).await;
    assert_eq!(result.outcome, Outcome::Ok);
    assert!(
        result.stats.http_bytes <= 256 * 1024,
        "{}",
        result.stats.http_bytes
    );
    assert!(result.pages[0].word_count > 1000);
}

#[tokio::test]
async fn deeply_nested_markup_does_not_crash() {
    let server = MockServer::start().await;
    let depth = 50_000;
    let body = format!(
        "<!doctype html><html><head><title>Deep</title></head><body>{}<p>bottom text</p>{}</body></html>",
        "<div>".repeat(depth),
        "</div>".repeat(depth)
    );
    mount_html(&server, "/", 200, body).await;
    let started = std::time::Instant::now();
    let result = run(&server, "/", options(RenderMode::Never)).await;
    assert_eq!(result.outcome, Outcome::Ok);
    // The HTML parser is quadratic in nesting depth; unbounded this took minutes.
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "{:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn malformed_css_is_survived() {
    let server = MockServer::start().await;
    mount_html(
        &server,
        "/",
        200,
        "<!doctype html><html><head><title>CSS</title><link rel=stylesheet href=/bad.css></head><body><h1>Hello there friends</h1></body></html>",
    )
    .await;
    let css = std::fs::read(fixtures().join("hostile/malformed.css")).unwrap();
    Mock::given(method("GET"))
        .and(path("/bad.css"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(css)
                .insert_header("content-type", "text/css"),
        )
        .mount(&server)
        .await;
    let result = run(&server, "/", options(RenderMode::Never)).await;
    assert_eq!(result.outcome, Outcome::Ok);
    let design = result.design.unwrap();
    assert_eq!(design.stylesheets.len(), 1);
    assert!(design
        .colors
        .iter()
        .all(|c| c.hex.len() == 7 && c.hex.starts_with('#')));
}

#[tokio::test]
async fn page_with_only_external_and_file_links_selects_nothing() {
    let server = MockServer::start().await;
    let body = std::fs::read(fixtures().join("hostile/links_only.html")).unwrap();
    mount_html(&server, "/", 200, body).await;
    let result = run(&server, "/", options(RenderMode::Never)).await;
    assert_eq!(result.outcome, Outcome::Ok);
    assert_eq!(result.pages.len(), 1);
    assert!(result.page_errors.is_empty());
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn non_html_homepage_is_not_html() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(vec![0x89, b'P', b'N', b'G'])
                .insert_header("content-type", "image/png"),
        )
        .mount(&server)
        .await;
    let result = run(&server, "/", options(RenderMode::Never)).await;
    assert_eq!(result.outcome, Outcome::NotHtml);
}

#[tokio::test]
async fn server_error_homepage_is_http_error() {
    let server = MockServer::start().await;
    mount_html(
        &server,
        "/",
        500,
        "<html><body>Internal Server Error</body></html>",
    )
    .await;
    let result = run(&server, "/", options(RenderMode::Never)).await;
    assert_eq!(result.outcome, Outcome::HttpError);
    assert_eq!(result.outcome_detail.as_deref(), Some("HTTP 500"));
}

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cli_extract_prints_one_json_document() {
    let server = MockServer::start().await;
    serve_site(&server, "bakery").await;
    let url = server.uri();
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new(env!("CARGO_BIN_EXE_blacksparrow"))
            .args([
                "extract",
                &url,
                "--allow-private-network",
                "--render",
                "never",
            ])
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["outcome"], "ok");
    assert_eq!(json["pages"].as_array().unwrap().len(), 4);
    assert_eq!(json["contacts"]["emails"][0], "hello@sweetcrumbs.com.bd");
}
