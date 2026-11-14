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
                // URL paths use `/` whatever the platform's separator.
                .replace('\\', "/");
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

/// The page asks for a web font, a video, a beacon and an image. Chromium is started with
/// `imagesEnabled=false`, so the image is never even requested; the other types are refused by
/// the request interceptor and must be counted and never reach the server, while the page and
/// its stylesheet load normally. (The metadata address is only blocked when private networks
/// are refused, and the test server itself lives on loopback, so that half is covered by the
/// guard tests and the unit tests of `guard.rs` instead of by weakening the policy here.)
#[tokio::test]
async fn rendered_page_requests_for_images_are_blocked() {
    let Some(opts) = chrome_options(RenderMode::Always) else {
        return;
    };
    let server = MockServer::start().await;
    mount_html(
        &server,
        "/",
        200,
        r#"<!doctype html><html lang="en"><head><title>Blocked resources</title>
<link rel="stylesheet" href="/site.css"></head>
<body><h1 class="font-user">Resources the renderer refuses</h1>
<img src="/pixel.png"><video src="/clip.mp4" preload="auto"></video>
<script>navigator.sendBeacon('/beacon', 'x');</script></body></html>"#,
    )
    .await;
    for (url_path, content_type, body) in [
        (
            "/site.css",
            "text/css",
            "@font-face { font-family: Probe; src: url(/probe.woff2) format('woff2'); }\
             .font-user { font-family: Probe, serif; }",
        ),
        ("/pixel.png", "image/png", "\0\0\0\0\0\0\0\0"),
        ("/probe.woff2", "font/woff2", "wOF2"),
        ("/clip.mp4", "video/mp4", "\0\0\0\0"),
        ("/beacon", "text/plain", "ok"),
    ] {
        Mock::given(method("GET"))
            .and(path(url_path))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_bytes(body.as_bytes().to_vec())
                    .insert_header("content-type", content_type),
            )
            .mount(&server)
            .await;
    }
    let result = run(&server, "/", opts).await;
    assert_eq!(result.outcome, Outcome::Ok);
    assert!(result.pages[0].rendered);
    assert!(
        result.stats.blocked_browser_requests >= 2,
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
        requested.iter().any(|p| p == "/site.css"),
        "the stylesheet should still load: {requested:?}"
    );
    for refused in ["/pixel.png", "/probe.woff2", "/clip.mp4", "/beacon"] {
        assert!(
            !requested.iter().any(|p| p == refused),
            "{refused} reached the server: {requested:?}"
        );
    }
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

const AWS_WAF_CAPTCHA: &str = r#"<html><head><title>Let's confirm you are human</title></head>
<body><script type="text/javascript">window.gokuProps = {"key":"abc","iv":"def","context":"ghi"};</script>
<script src="https://1234.edge.sdk.awswaf.com/1234/challenge.js"></script>
<script src="https://1234.edge.sdk.awswaf.com/1234/captcha.js"></script>
<div id="captcha-container"></div>
<script>AwsWafIntegration.checkForceRefresh();</script>
<noscript>JavaScript is disabled. Enable it and solve the CAPTCHA puzzle to continue.</noscript>
<script>window.addEventListener("load", function () { AwsWafCaptcha.renderCaptcha(document.getElementById("captcha-container")); });</script>
<p>captcha.awswaf.com</p></body></html>"#;

async fn mount_aws_waf(server: &MockServer, status: u16, action: &str) {
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(status)
                .set_body_bytes(AWS_WAF_CAPTCHA)
                .insert_header("content-type", "text/html; charset=utf-8")
                .insert_header("x-amzn-waf-action", action)
                .insert_header("server", "CloudFront"),
        )
        .mount(server)
        .await;
}

#[tokio::test]
async fn aws_waf_captcha_page_is_bot_protected() {
    let server = MockServer::start().await;
    mount_aws_waf(&server, 405, "captcha").await;
    let result = run(&server, "/", options(RenderMode::Never)).await;
    assert_eq!(result.outcome, Outcome::BotProtected);
    assert_eq!(result.outcome_detail.as_deref(), Some("AWS WAF"));
    assert_eq!(result.pages[0].waf, Some("AWS WAF"));
    assert_eq!(result.pages[0].status, 405);
}

#[tokio::test]
async fn aws_waf_js_challenge_without_markers_is_bot_protected() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(202)
                .set_body_bytes("<html><body></body></html>")
                .insert_header("content-type", "text/html; charset=utf-8")
                .insert_header("x-amzn-waf-action", "challenge"),
        )
        .mount(&server)
        .await;
    let result = run(&server, "/", options(RenderMode::Never)).await;
    assert_eq!(result.outcome, Outcome::BotProtected);
    assert_eq!(result.outcome_detail.as_deref(), Some("AWS WAF"));
}

#[tokio::test]
async fn aws_waf_captcha_stays_bot_protected_after_rendering() {
    let Some(opts) = chrome_options(RenderMode::Auto) else {
        return;
    };
    let server = MockServer::start().await;
    mount_aws_waf(&server, 405, "captcha").await;
    let result = run(&server, "/", opts).await;
    assert_eq!(result.outcome, Outcome::BotProtected);
    assert_eq!(result.outcome_detail.as_deref(), Some("AWS WAF"));
    // The browser asked for the page itself instead of reusing the downloaded body.
    assert!(server.received_requests().await.unwrap().len() >= 2);
}

#[tokio::test]
async fn error_homepage_is_requested_again_by_the_browser_and_stays_an_error() {
    let Some(opts) = chrome_options(RenderMode::Auto) else {
        return;
    };
    let server = MockServer::start().await;
    mount_html(
        &server,
        "/",
        404,
        r#"<html><body><div id="root"></div><script>var a = 1;</script></body></html>"#,
    )
    .await;
    let result = run(&server, "/", opts).await;
    assert_eq!(result.outcome, Outcome::HttpError);
    assert_eq!(result.outcome_detail.as_deref(), Some("HTTP 404"));
    assert!(server.received_requests().await.unwrap().len() >= 2);
}

#[tokio::test]
async fn empty_homepage_is_rendered_when_auto_and_stays_ok_with_no_words() {
    let Some(opts) = chrome_options(RenderMode::Auto) else {
        return;
    };
    let server = MockServer::start().await;
    mount_html(&server, "/", 200, "").await;
    let result = run(&server, "/", opts).await;
    assert_eq!(result.outcome, Outcome::Ok);
    let home = &result.pages[0];
    assert!(home.rendered);
    assert_eq!(home.render_reason, Some("js_shell"));
    assert_eq!(home.word_count, 0);
}

#[tokio::test]
async fn empty_homepage_without_rendering_is_ok_with_no_words() {
    let server = MockServer::start().await;
    mount_html(&server, "/", 200, "").await;
    let result = run(&server, "/", options(RenderMode::Never)).await;
    assert_eq!(result.outcome, Outcome::Ok);
    assert_eq!(result.pages[0].word_count, 0);
    assert!(!result.pages[0].rendered);
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

#[tokio::test]
async fn audit_render_js_crawls_links_that_only_exist_after_scripts() {
    let Ok(chrome) = std::env::var("BLACKSPARROW_TEST_CHROME") else {
        eprintln!("skipped: BLACKSPARROW_TEST_CHROME is not set");
        return;
    };
    let server = MockServer::start().await;
    serve_site(&server, "spa").await;
    let mut config = blacksparrow::core::config::CrawlConfig::new(&server.uri()).unwrap();
    config.max_pages = 5;
    config.ephemeral = true;
    config.respect_robots = false;
    config.quiet = true;

    let plain = blacksparrow::crawler::engine::run_crawl(&config, None)
        .await
        .unwrap();
    assert_eq!(
        plain.pages.len(),
        1,
        "the shell links nowhere without scripts"
    );

    config.render_js = true;
    config.chrome_path = Some(PathBuf::from(chrome));
    config.chrome_no_sandbox = true;
    let rendered = blacksparrow::crawler::engine::run_crawl(&config, None)
        .await
        .unwrap();
    assert!(
        rendered.pages.len() >= 2,
        "the rendered page links to its contact page"
    );
    assert!(rendered.pages[0].word_count > 20);
}

// ---------------------------------------------------------------------------
// Review regressions: encodings, false-positive contacts, CSS colours, logos
// ---------------------------------------------------------------------------

#[tokio::test]
async fn legacy_encoded_pages_and_stylesheets_are_decoded() {
    let server = MockServer::start().await;
    // windows-1252 page declaring its charset only in a meta tag, with a stylesheet whose
    // `@charset` is also windows-1252 and a font name containing a non-ASCII letter.
    let page = b"<!doctype html><html><head><meta http-equiv=\"Content-Type\" content=\"text/html; charset=windows-1252\"><title>Caf\xe9 R\xf6st</title><link rel=stylesheet href=/s.css></head><body><h1>Welcome to Caf\xe9 R\xf6st, a small caf\xe9 in Dhaka</h1></body></html>".to_vec();
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(page)
                .insert_header("content-type", "text/html"),
        )
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/s.css"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(
                    b"@charset \"windows-1252\"; h1 { font-family: 'Caf\xe9 Sans'; }".to_vec(),
                )
                .insert_header("content-type", "text/css"),
        )
        .mount(&server)
        .await;
    let result = run(&server, "/", options(RenderMode::Never)).await;
    let home = &result.pages[0];
    assert_eq!(home.title.as_deref(), Some("Café Röst"));
    assert!(home.markdown.contains("Welcome to Café Röst"));
    let design = result.design.unwrap();
    assert!(design.fonts.iter().any(|f| f.family == "Café Sans"));
}

#[tokio::test]
async fn contacts_ignore_dates_hours_and_lookalike_links() {
    let server = MockServer::start().await;
    let html = r#"<!doctype html><html><head><title>Hotel Sunrise</title></head><body>
        <h1>Hotel Sunrise, serving guests since 2015-2020 and beyond</h1>
        <p>Open 9.00-21.00 every day. Booking calling hours 2024-05-12.</p>
        <p>Phone no 0961-2345678</p>
        <p>Hotline No 16516</p>
        <a href="tel:+880%201711%20223344">Call</a>
        <a href="https://www.facebook.com/people/Sunrise-Hotel/100012345/">fb</a>
        <a href="https://www.facebook.com/sharer/sharer.php?u=x">share</a>
        <a href="https://www.facebook.com/like.php?href=x">like</a>
        <a href="https://www.instagram.com/share/abc">share</a>
        <p>Mail: front[at]sunrise.test or image@2x.png</p>
        </body></html>"#;
    mount_html(&server, "/", 200, html).await;
    let result = run(&server, "/", options(RenderMode::Never)).await;
    let contacts = result.contacts;
    assert_eq!(
        contacts.phones,
        vec!["+880 1711 223344", "0961-2345678", "16516"]
    );
    assert_eq!(contacts.emails, vec!["front@sunrise.test"]);
    assert_eq!(
        contacts.facebook,
        vec!["https://www.facebook.com/people/Sunrise-Hotel/100012345"]
    );
    assert!(contacts.instagram.is_empty());
}

#[tokio::test]
async fn shadow_and_transparent_colours_do_not_rank_and_svg_logos_are_sanitized() {
    let server = MockServer::start().await;
    let html = r##"<!doctype html><html><head><title>Brand</title><style>
        :root { --brand: #E11D48; }
        .a { color: var(--brand); } .b { background: var(--brand); }
        .c { box-shadow: 0 1px 2px rgba(0,0,0,.1); } .d { box-shadow: 0 0 3px #000; }
        .e { border-color: rgba(0,0,0,.08); } .f { text-shadow: 1px 1px #000; }
        </style></head><body><header><a class="logo" href="/">
        <svg viewBox="0 0 10 10" onload="alert(1)"><script>alert(1)</script>
        <foreignObject><div>x</div></foreignObject><path d="M0 0h10"/></svg></a></header>
        <h1>Brand</h1></body></html>"##;
    mount_html(&server, "/", 200, html).await;
    let result = run(&server, "/", options(RenderMode::Never)).await;
    let design = result.design.unwrap();
    assert_eq!(design.colors[0].hex, "#E11D48");
    assert!(design.colors.iter().all(|c| c.hex != "#000000"));
    let svg = design.logos[0].svg.as_deref().unwrap();
    assert!(svg.contains("<path d=\"M0 0h10\">"));
    assert!(!svg.contains("script") && !svg.contains("onload") && !svg.contains("foreignObject"));
}
