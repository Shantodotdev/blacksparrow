//! # Smart Page Intent Classifier & Contextual Schema Integration Tests
//!
//! Validates:
//! 1. 6-Archetype evidential intent classification (Product, Article, Category, Contact, Homepage, Standard).
//! 2. Adversarial div-soup resilience (zero semantic tags, pure Tailwind/flex <div> elements).
//! 3. Spatial Transactional Micro-Clusters (proximity between price, quantity input, and multilingual CTA).
//! 4. Multilingual intent extraction (German, French, Spanish, Bengali, etc.).
//! 5. Chrome DevTools Protocol (CDP) signal fusion for Client-Side Rendered (CSR) SPAs.
//! 6. Contextual schema audit rules:
//!    - `ERR_PRODUCT_MISSING_SCHEMA`, `WARN_PRODUCT_MISSING_PRICE_OFFER`, `WARN_PRODUCT_MISSING_AVAILABILITY`
//!    - `ERR_ARTICLE_MISSING_SCHEMA`, `WARN_ARTICLE_MISSING_AUTHOR`, `WARN_ARTICLE_MISSING_DATE_PUBLISHED`
//!    - `WARN_ORG_MISSING_LOCAL_SCHEMA`
//! 7. Live internet verification.

use seo_lens::core::models::{PageArchetype, RuleId};
use seo_lens::crawler::client::FetchResult;
use seo_lens::parser::intent::{classify_intent, CdpIntentSignals};
use seo_lens::parser::parse_html;
use seo_lens::rules::evaluate_page;

#[test]
fn test_archetype_enum_and_display() {
    assert_eq!(PageArchetype::Standard.as_str(), "standard");
    assert_eq!(PageArchetype::Product.as_str(), "product");
    assert_eq!(PageArchetype::Article.as_str(), "article");
    assert_eq!(PageArchetype::Category.as_str(), "category");
    assert_eq!(PageArchetype::Contact.as_str(), "contact");
    assert_eq!(PageArchetype::Homepage.as_str(), "homepage");

    assert!(PageArchetype::Product.badge().contains("Product"));
    assert!(PageArchetype::Article.badge().contains("Article"));
}

#[test]
fn test_adversarial_div_soup_product_intent() {
    // Pure div-soup with German CTA, price, and quantity input in un-semantic <div> tags
    let html = r#"
        <div class="x71 v09">
            <div class="flex-col">
                <div class="text-xl font-bold">Ergonomischer Bürostuhl Pro</div>
                <div class="price-row">
                    <span class="curr">€</span><span class="val">299,99</span>
                    <span class="stock-badge">Auf Lager</span>
                </div>
                <div class="ctrls">
                    <input type="number" name="quantity" value="1" min="1" max="10">
                    <button type="submit" class="btn-cta">In den Warenkorb</button>
                </div>
                <div class="details">
                    Atmungsaktiver Netzstoff, verstellbare Lordosenstütze und 3D-Armlehnen.
                </div>
            </div>
        </div>
    "#;

    let page = parse_html(html, "https://shop.example.de/stuhl-pro").unwrap();
    let intent = classify_intent(html, "https://shop.example.de/stuhl-pro", &page, None);

    assert_eq!(intent.archetype, PageArchetype::Product);
    assert!(
        intent.confidence > 0.85,
        "Expected confidence > 0.85, got {}",
        intent.confidence
    );
    assert!(
        intent
            .signals
            .iter()
            .any(|s| s.contains("transactional_micro_cluster")),
        "Expected transactional micro-cluster signal, got: {:?}",
        intent.signals
    );
}

#[test]
fn test_adversarial_div_soup_article_intent() {
    let prose = "Rust's ownership model provides memory safety without a garbage collector. \
        By tracking reference lifetimes at compile time, the borrow checker eliminates data races, \
        use-after-free bugs, and double-free vulnerabilities. This unique combination makes Rust \
        ideal for systems programming, high-throughput network services, and performance-critical engines. ".repeat(6);

    let html = format!(
        r#"
        <div class="page-container">
            <div class="header-box">
                <div class="title-text">Deep Dive into Memory Safety and Borrowing</div>
                <div class="meta-row">
                    <span class="byline">By Dr. Elizabeth Shaw</span>
                    <span class="dot">•</span>
                    <span class="date">Published on March 15, 2025</span>
                </div>
            </div>
            <div class="body-content">
                <div class="paragraph">{prose}</div>
            </div>
        </div>
    "#
    );

    let page = parse_html(
        &html,
        "https://techblog.example.org/articles/memory-safety-rust",
    )
    .unwrap();
    let intent = classify_intent(
        &html,
        "https://techblog.example.org/articles/memory-safety-rust",
        &page,
        None,
    );

    assert_eq!(intent.archetype, PageArchetype::Article);
    assert!(
        intent.confidence > 0.85,
        "Expected confidence > 0.85, got {}",
        intent.confidence
    );
    assert!(
        intent
            .signals
            .iter()
            .any(|s| s.contains("continuous_narrative_runs")),
        "Expected continuous narrative signal, got: {:?}",
        intent.signals
    );
}

#[test]
fn test_adversarial_div_soup_category_catalog_intent() {
    let mut items = String::new();
    for i in 1..=10 {
        items.push_str(&format!(
            r#"
            <div class="grid-card">
                <img src="/img/shoe{i}.jpg" alt="Running Shoe Model {i}">
                <a href="/shoes/model-{i}">Ultra Runner Pro {i}</a>
                <div class="price">${}.99</div>
            </div>
        "#,
            79 + i * 5
        ));
    }

    let html = format!(
        r#"
        <div class="catalog-view">
            <div class="banner">Men's Running Footwear</div>
            <div class="items-grid">
                {items}
            </div>
            <div class="pagination-bar">
                <a href="/shoes/mens?page=1">1</a>
                <a href="/shoes/mens?page=2" rel="next">2</a>
                <a href="/shoes/mens?page=3">3</a>
            </div>
        </div>
    "#
    );

    let page = parse_html(&html, "https://store.example.com/shoes/mens").unwrap();
    let intent = classify_intent(&html, "https://store.example.com/shoes/mens", &page, None);

    assert_eq!(intent.archetype, PageArchetype::Category);
    assert!(
        intent.confidence > 0.85,
        "Expected confidence > 0.85, got {}",
        intent.confidence
    );
    assert!(
        intent
            .signals
            .iter()
            .any(|s| s.contains("multi_price_catalog_grid")),
        "Expected catalog grid signal, got: {:?}",
        intent.signals
    );
}

#[test]
fn test_adversarial_div_soup_contact_intent() {
    let html = r#"
        <div class="contact-shell">
            <div class="headline">Get In Touch With Our Team</div>
            <div class="phone-link">Call us directly: <a href="tel:+18005550199">+1 (800) 555-0199</a></div>
            <div class="form-wrapper">
                <form action="/send-message" method="POST">
                    <input type="text" name="fullName" placeholder="Your Name">
                    <input type="email" name="user_email" placeholder="Email Address">
                    <textarea name="message" placeholder="How can we help?"></textarea>
                    <button type="submit">Send Message</button>
                </form>
            </div>
        </div>
    "#;

    let page = parse_html(html, "https://company.example.com/contact-us").unwrap();
    let intent = classify_intent(html, "https://company.example.com/contact-us", &page, None);

    assert_eq!(intent.archetype, PageArchetype::Contact);
    assert!(
        intent.confidence > 0.85,
        "Expected confidence > 0.85, got {}",
        intent.confidence
    );
    assert!(
        intent.signals.iter().any(|s| s.contains("contact_form")),
        "Expected contact form signal, got: {:?}",
        intent.signals
    );
}

#[test]
fn test_root_homepage_intent() {
    let html = r#"
        <!DOCTYPE html>
        <html>
        <head><title>Apex Systems - High Performance Cloud</title></head>
        <body>
            <header>
                <a href="/">Apex Home</a>
                <nav>
                    <a href="/products">Products</a>
                    <a href="/solutions">Solutions</a>
                    <a href="/pricing">Pricing</a>
                    <a href="/contact">Contact</a>
                </nav>
            </header>
            <main>
                <h1>Next Generation Cloud Infrastructure</h1>
                <p>Deploy globally in seconds with distributed edge computing.</p>
                <a href="/signup">Get Started Free</a>
            </main>
        </body>
        </html>
    "#;

    let page = parse_html(html, "https://apexsystems.io/").unwrap();
    let intent = classify_intent(html, "https://apexsystems.io/", &page, None);

    assert_eq!(intent.archetype, PageArchetype::Homepage);
    assert!(
        intent.confidence > 0.90,
        "Expected confidence > 0.90, got {}",
        intent.confidence
    );
}

#[test]
fn test_docs_and_legal_classified_as_standard() {
    let html = r#"
        <!DOCTYPE html>
        <html>
        <head><title>Privacy Policy | Apex Systems</title></head>
        <body>
            <h1>Privacy Policy</h1>
            <p>Last updated: January 2025. This privacy policy explains how we collect and use your personal information.</p>
            <p>We respect your privacy rights and handle all personal data in compliance with GDPR and CCPA regulations.</p>
        </body>
        </html>
    "#;

    let page = parse_html(html, "https://apexsystems.io/legal/privacy").unwrap();
    let intent = classify_intent(html, "https://apexsystems.io/legal/privacy", &page, None);

    assert_eq!(intent.archetype, PageArchetype::Standard);
    assert!(
        intent.confidence > 0.80,
        "Expected confidence > 0.80, got {}",
        intent.confidence
    );
}

#[test]
fn test_multilingual_bengali_product() {
    let html = r#"
        <div class="product-box">
            <h1>প্রিমিয়াম লেদার ওয়ালেট</h1>
            <div class="price-tag">মূল্য: ৳১,২৫০</div>
            <div class="stock">স্টকে আছে</div>
            <div class="buy-section">
                <input type="number" name="qty" value="1">
                <button type="submit">কার্টে যোগ করুন</button>
            </div>
            <p>১০০% খাঁটি চামড়া দিয়ে তৈরি প্রিমিয়াম মানিব্যাগ। দীর্ঘস্থায়ী এবং মার্জিত ডিজাইন।</p>
        </div>
    "#;

    let page = parse_html(html, "https://daraz.com.bd/products/leather-wallet").unwrap();
    let intent = classify_intent(
        html,
        "https://daraz.com.bd/products/leather-wallet",
        &page,
        None,
    );

    assert_eq!(intent.archetype, PageArchetype::Product);
    assert!(
        intent.confidence > 0.85,
        "Expected confidence > 0.85, got {}",
        intent.confidence
    );
}

#[test]
fn test_cdp_signal_fusion_for_csr_spa() {
    // Initial raw HTML is completely blank CSR shell
    let raw_html = r#"<!DOCTYPE html><html><head><title>Apex App</title></head><body><div id="root"></div></body></html>"#;
    let page = parse_html(raw_html, "https://spa.example.com/item/123").unwrap();

    let cdp_signals = CdpIntentSignals {
        is_platform_pdp: true,
        hero_price_above_fold: true,
        hero_price_font_size: 28.0,
        uniform_card_count: 1,
        rendered_html: None,
        ..Default::default()
    };

    let intent = classify_intent(
        raw_html,
        "https://spa.example.com/item/123",
        &page,
        Some(&cdp_signals),
    );

    assert_eq!(intent.archetype, PageArchetype::Product);
    assert!(
        intent.confidence > 0.95,
        "Expected confidence > 0.95, got {}",
        intent.confidence
    );
    assert!(
        intent
            .signals
            .iter()
            .any(|s| s.contains("cdp_runtime_platform_pdp")),
        "Expected CDP platform PDP signal, got: {:?}",
        intent.signals
    );
}

// =========================================================================
// Contextual Schema Audit Rules Tests
// =========================================================================

fn make_mock_fetch(url: &str, body: &str) -> FetchResult {
    FetchResult {
        url: url.to_string(),
        final_url: url.to_string(),
        status_code: 200,
        content_type: compact_str::CompactString::new("text/html; charset=utf-8"),
        headers: reqwest::header::HeaderMap::new(),
        body: body.to_string(),
        size_bytes: body.len() as u32,
        ttfb_ms: 45,
        redirect_chain: Vec::new(),
        body_bytes: Vec::new(),
        waf_detected: None,
    }
}

#[test]
fn test_rule_err_product_missing_schema() {
    let html = r#"
        <html>
        <body>
            <h1>Titanium Mechanical Watch</h1>
            <span>$499.00</span>
            <input type="number" name="qty" value="1">
            <button>Add to Cart</button>
        </body>
        </html>
    "#;

    let page = parse_html(html, "https://store.example.com/products/watch").unwrap();
    assert_eq!(page.page_intent.archetype, PageArchetype::Product);

    let fetch = make_mock_fetch("https://store.example.com/products/watch", html);
    let issues = evaluate_page(&page, &fetch);

    assert!(
        issues
            .iter()
            .any(|i| i.code == RuleId::ErrProductMissingSchema),
        "Expected ERR_PRODUCT_MISSING_SCHEMA to trigger, found: {:?}",
        issues.iter().map(|i| i.code).collect::<Vec<_>>()
    );
}

#[test]
fn test_rule_warn_product_missing_price_offer_and_availability() {
    let html = r#"
        <html>
        <head>
            <script type="application/ld+json">
            {
                "@context": "https://schema.org",
                "@type": "Product",
                "name": "Titanium Mechanical Watch",
                "description": "Luxury automatic watch"
            }
            </script>
        </head>
        <body>
            <h1>Titanium Mechanical Watch</h1>
            <span>$499.00</span>
            <input type="number" name="qty" value="1">
            <button>Add to Cart</button>
        </body>
        </html>
    "#;

    let page = parse_html(html, "https://store.example.com/products/watch").unwrap();
    assert_eq!(page.page_intent.archetype, PageArchetype::Product);

    let fetch = make_mock_fetch("https://store.example.com/products/watch", html);
    let issues = evaluate_page(&page, &fetch);

    assert!(
        issues
            .iter()
            .any(|i| i.code == RuleId::WarnProductMissingPriceOffer),
        "Expected WARN_PRODUCT_MISSING_PRICE_OFFER to trigger"
    );
    assert!(
        issues
            .iter()
            .any(|i| i.code == RuleId::WarnProductMissingAvailability),
        "Expected WARN_PRODUCT_MISSING_AVAILABILITY to trigger"
    );
}

#[test]
fn test_rule_err_article_missing_schema() {
    let prose = "Rust concurrency ensures threads can share state safely without data races. \
        The Send and Sync traits are fundamental building blocks of Rust's fearless concurrency. "
        .repeat(6);

    let html = format!(
        r#"
        <html>
        <body>
            <h1>Understanding Fearless Concurrency in Rust</h1>
            <p>By Alex Rivera • Published on January 10, 2025</p>
            <article>{prose}</article>
        </body>
        </html>
    "#
    );

    let page = parse_html(&html, "https://blog.example.com/posts/fearless-concurrency").unwrap();
    assert_eq!(page.page_intent.archetype, PageArchetype::Article);

    let fetch = make_mock_fetch("https://blog.example.com/posts/fearless-concurrency", &html);
    let issues = evaluate_page(&page, &fetch);

    assert!(
        issues
            .iter()
            .any(|i| i.code == RuleId::ErrArticleMissingSchema),
        "Expected ERR_ARTICLE_MISSING_SCHEMA to trigger"
    );
}

#[test]
fn test_rule_warn_article_missing_author_and_date() {
    let prose =
        "Understanding garbage collection versus manual memory management in modern computing. "
            .repeat(8);

    let html = format!(
        r#"
        <html>
        <head>
            <script type="application/ld+json">
            {{
                "@context": "https://schema.org",
                "@type": "Article",
                "headline": "Memory Management in 2025"
            }}
            </script>
        </head>
        <body>
            <h1>Memory Management in 2025</h1>
            <p>By Alex Rivera • Published on January 10, 2025</p>
            <article>{prose}</article>
        </body>
        </html>
    "#
    );

    let page = parse_html(&html, "https://blog.example.com/posts/memory-mgmt").unwrap();
    assert_eq!(page.page_intent.archetype, PageArchetype::Article);

    let fetch = make_mock_fetch("https://blog.example.com/posts/memory-mgmt", &html);
    let issues = evaluate_page(&page, &fetch);

    assert!(
        issues
            .iter()
            .any(|i| i.code == RuleId::WarnArticleMissingAuthor),
        "Expected WARN_ARTICLE_MISSING_AUTHOR to trigger"
    );
    assert!(
        issues
            .iter()
            .any(|i| i.code == RuleId::WarnArticleMissingDatePublished),
        "Expected WARN_ARTICLE_MISSING_DATE_PUBLISHED to trigger"
    );
}

#[test]
fn test_rule_warn_org_missing_local_schema() {
    let html = r#"
        <html>
        <body>
            <h1>Contact Us</h1>
            <p>Call us at <a href="tel:+18005550199">+1 (800) 555-0199</a></p>
            <form action="/contact" method="POST">
                <input type="email" name="email">
                <textarea name="message"></textarea>
                <button type="submit">Submit</button>
            </form>
        </body>
        </html>
    "#;

    let page = parse_html(html, "https://company.example.com/contact").unwrap();
    assert_eq!(page.page_intent.archetype, PageArchetype::Contact);

    let fetch = make_mock_fetch("https://company.example.com/contact", html);
    let issues = evaluate_page(&page, &fetch);

    assert!(
        issues
            .iter()
            .any(|i| i.code == RuleId::WarnOrgMissingLocalSchema),
        "Expected WARN_ORG_MISSING_LOCAL_SCHEMA to trigger"
    );
}

#[tokio::test]
async fn test_live_url_intent_classification() {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap();

    // 1. Homepage
    if let Ok(resp) = client.get("https://books.toscrape.com/").send().await {
        if resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            let page = parse_html(&body, "https://books.toscrape.com/").unwrap();
            assert_eq!(
                page.page_intent.archetype,
                PageArchetype::Homepage,
                "Root books.toscrape.com must be classified as Homepage"
            );
        }
    }

    // 2. Product Detail Page
    let product_url = "https://books.toscrape.com/catalogue/a-light-in-the-attic_1000/index.html";
    if let Ok(resp) = client.get(product_url).send().await {
        if resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            let page = parse_html(&body, product_url).unwrap();
            assert_eq!(
                page.page_intent.archetype,
                PageArchetype::Product,
                "Book detail page on books.toscrape.com must be classified as Product"
            );
        }
    }
}

#[test]
fn test_b2b_industrial_product_intent_without_cart() {
    // Industrial equipment page with WhatsApp CTA, tel link, hero price, spec dl table,
    // and related products footer > 3500 bytes later, without a standard "Add to cart" button.
    let filler_desc = "<p>Heavy duty CNC precision machinery designed for industrial aerospace and automotive manufacturing. Robust cast iron bed ensures maximum vibration dampening and thermal stability.</p>".repeat(25);

    let html = format!(
        r#"
        <html>
        <head><title>CNC Precision Lathe Cutter 5000 | Industrial Machinery</title></head>
        <body>
            <div class="breadcrumb"><a href="/products">Products</a> &gt; CNC Lathe</div>
            <h1>CNC Precision Lathe Cutter 5000</h1>
            <div class="price-box">
                <span class="currency">$</span><span class="amount">45,000</span>
            </div>
            <div class="inquiry-actions">
                <a href="https://wa.me/18005550199?text=Inquiry%20CNC%205000" class="wa-btn">Inquire on WhatsApp</a>
                <a href="tel:+18005550199" class="call-btn">Call Technical Sales</a>
            </div>
            <div class="specs-section">
                <h2>Technical Specifications</h2>
                <dl class="spec-matrix">
                    <dt>Spindle Speed</dt><dd>10 - 4500 RPM</dd>
                    <dt>Motor Power</dt><dd>15 kW High Torque</dd>
                    <dt>Chuck Diameter</dt><dd>250 mm</dd>
                    <dt>Max Turning Length</dt><dd>1000 mm</dd>
                    <dt>Positioning Accuracy</dt><dd>±0.005 mm</dd>
                    <dt>Machine Weight</dt><dd>4800 kg</dd>
                </dl>
            </div>
            <div class="detailed-description">
                {filler_desc}
            </div>
            <div class="related-products-footer">
                <h3>Related Machinery</h3>
                <div class="related-card">
                    <a href="/products/cnc-milling-center">CNC Milling Center 3000</a>
                    <span>$38,000</span>
                </div>
                <div class="related-card">
                    <a href="/products/cnc-5axis-lathe">5-Axis CNC Lathe</a>
                    <span>$52,000</span>
                </div>
                <div class="related-card">
                    <a href="/products/laser-cutting-system">Fiber Laser 6kW</a>
                    <span>$61,000</span>
                </div>
            </div>
        </body>
        </html>
        "#
    );

    let url = "https://example-machinery.com/products/cnc-precision-lathe-cutter-5000";
    let page = parse_html(&html, url).unwrap();

    assert_eq!(
        page.page_intent.archetype,
        PageArchetype::Product,
        "B2B industrial page with specs, protocol CTA, and hero price must be classified as Product"
    );
    assert!(
        page.page_intent.confidence > 0.80,
        "Expected high confidence, got {}",
        page.page_intent.confidence
    );

    let fetch = make_mock_fetch(url, &html);
    let issues = evaluate_page(&page, &fetch);

    assert!(
        issues
            .iter()
            .any(|i| i.code == RuleId::ErrProductMissingSchema),
        "Expected ERR_PRODUCT_MISSING_SCHEMA for product missing Schema.org Product data"
    );
}

#[tokio::test]
async fn test_live_url_starelevator_b2b_product() {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap();

    let url = "https://www.starelevatorltd.com/products/1600kg-hospital-bed-stretcher-elevator";
    if let Ok(resp) = client.get(url).send().await {
        if resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            let page = parse_html(&body, url).unwrap();

            assert_eq!(
                page.page_intent.archetype,
                PageArchetype::Product,
                "Star Elevator medical bed elevator page must be classified as Product"
            );

            let fetch = make_mock_fetch(url, &body);
            let issues = evaluate_page(&page, &fetch);

            assert!(
                issues
                    .iter()
                    .any(|i| i.code == RuleId::ErrProductMissingSchema),
                "Expected ERR_PRODUCT_MISSING_SCHEMA for starelevatorltd product page lacking JSON-LD"
            );
        }
    }
}
