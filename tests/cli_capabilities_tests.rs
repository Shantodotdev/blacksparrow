//! # CLI Capabilities & Extended Commands Integration Tests (Micro-Phase 01)
//!
//! Validates new subcommands (`issues`, `check-ai`, `delete`, `clean`, `schema`),
//! enriched flags (`--include`, `--exclude`, `--header`, `--quiet`, `--format`),
//! database cascade deletions, and AI readiness auditing.

use blacksparrow::cli::args::{Cli, Commands};
use blacksparrow::core::config::CrawlConfig;
use blacksparrow::core::models::{
    IssueCategory, IssueFinding, PageReport, RobotsFlags, RuleId, Severity,
};
use blacksparrow::crawler::ai_check::{audit_ai_readiness, AiSearchRisk};
use blacksparrow::crawler::engine::run_crawl;
use blacksparrow::rules::page::schema_val::validate_raw_schema;
use blacksparrow::storage::{CrawlSessionInit, Database, IssueFilterCriteria};
use clap::Parser;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

static TEST_COUNTER: AtomicUsize = AtomicUsize::new(100);

fn unique_test_db_path() -> PathBuf {
    let id = TEST_COUNTER.fetch_add(1, Ordering::SeqCst);
    let mut dir = std::env::temp_dir();
    dir.push(format!("seolens_cli_test_{}_{}.db", std::process::id(), id));
    dir
}

fn mock_page_report(crawl_id: &str, url: &str, status_code: u16) -> PageReport {
    PageReport {
        crawl_id: crawl_id.into(),
        url: url.to_string(),
        url_hash: blacksparrow::core::url::url_hash(url),
        final_url: Some(url.to_string()),
        status_code,
        content_type: "text/html; charset=utf-8".into(),
        size_bytes: 1024,
        ttfb_ms: 80,
        crawl_depth: 1,
        title: Some("Test Page".to_string()),
        title_length: 9,
        meta_description: Some("Test description".to_string()),
        meta_desc_length: 16,
        canonical_url: Some(url.to_string()),
        html_lang: Some("en".into()),
        robots_flags: RobotsFlags::NONE,
        is_sitemap_url: false,
        is_internal: true,
        h1_primary: Some("Heading".to_string()),
        h1_count: 1,
        h2_headings: vec![],
        h3_headings: vec![],
        word_count: 250,
        content_hash: 111,
        simhash: 222,
        is_https: true,
        has_hsts: true,
        has_csp: true,
        has_x_frame: true,
        has_x_content_type: true,
        links: vec![],
        images: vec![],
        schemas: vec![],
        hreflangs: vec![],
        issues: vec![],
        page_intent: Default::default(),
        ..Default::default()
    }
}

#[test]
fn test_cli_argument_parsing_new_commands() {
    // 1. issues subcommand
    let cli = Cli::try_parse_from([
        "seolens",
        "issues",
        "session_abc",
        "--severity",
        "critical",
        "--category",
        "security",
        "--limit",
        "25",
        "--format",
        "json",
    ])
    .expect("Failed to parse issues command");

    match cli.command {
        Commands::Issues(args) => {
            assert_eq!(
                args.session.unwrap_or(args.session_pos.unwrap()),
                "session_abc"
            );
            assert_eq!(args.severity.as_deref(), Some("critical"));
            assert_eq!(args.category.as_deref(), Some("security"));
            assert_eq!(args.limit, 25);
            assert_eq!(args.format, "json");
        }
        _ => panic!("Expected Issues command"),
    }

    // 2. check-ai subcommand
    let cli = Cli::try_parse_from([
        "seolens",
        "check-ai",
        "https://example.com",
        "--format",
        "json",
        "--timeout",
        "10",
    ])
    .expect("Failed to parse check-ai command");

    match cli.command {
        Commands::CheckAi(args) => {
            assert_eq!(args.url, "https://example.com");
            assert_eq!(args.format, "json");
            assert_eq!(args.timeout, 10);
        }
        _ => panic!("Expected CheckAi command"),
    }

    // 3. delete subcommand
    let cli = Cli::try_parse_from(["seolens", "delete", "session_to_delete"])
        .expect("Failed to parse delete command");

    match cli.command {
        Commands::Delete(args) => {
            assert_eq!(
                args.session.unwrap_or(args.session_pos.unwrap()),
                "session_to_delete"
            );
        }
        _ => panic!("Expected Delete command"),
    }

    // 4. clean subcommand
    let cli = Cli::try_parse_from(["seolens", "clean", "--older-than", "14"])
        .expect("Failed to parse clean command");

    match cli.command {
        Commands::Clean(args) => {
            assert_eq!(args.older_than, Some(14));
            assert!(!args.all);
        }
        _ => panic!("Expected Clean command"),
    }

    // 5. schema subcommand
    let cli = Cli::try_parse_from([
        "seolens",
        "schema",
        "https://example.com/product",
        "--type",
        "Product",
        "--format",
        "json",
    ])
    .expect("Failed to parse schema command");

    match cli.command {
        Commands::Schema(args) => {
            assert_eq!(args.target, "https://example.com/product");
            assert_eq!(args.expected_type.as_deref(), Some("Product"));
            assert_eq!(args.format, "json");
        }
        _ => panic!("Expected Schema command"),
    }
}

#[test]
fn test_cli_argument_parsing_extended_flags() {
    let cli = Cli::try_parse_from([
        "seolens",
        "audit",
        "https://example.com",
        "--include",
        "^/docs/.*",
        "--exclude",
        "^/docs/v1/.*",
        "-H",
        "Authorization: Bearer secret",
        "-H",
        "CF-Access-Client-Id: 12345",
        "--sitemap",
        "https://example.com/custom-sitemap.xml",
        "--name",
        "Docs Audit Q3",
        "-q",
    ])
    .expect("Failed to parse enriched audit command");

    match cli.command {
        Commands::Audit(args) => {
            assert_eq!(args.include.as_deref(), Some("^/docs/.*"));
            assert_eq!(args.exclude.as_deref(), Some("^/docs/v1/.*"));
            assert_eq!(args.headers.len(), 2);
            assert_eq!(args.headers[0], "Authorization: Bearer secret");
            assert_eq!(args.headers[1], "CF-Access-Client-Id: 12345");
            assert_eq!(
                args.sitemap.as_deref(),
                Some("https://example.com/custom-sitemap.xml")
            );
            assert_eq!(args.name.as_deref(), Some("Docs Audit Q3"));
            assert!(args.quiet);
        }
        _ => panic!("Expected Audit command"),
    }
}

#[test]
fn test_storage_delete_and_clean_cascading() {
    let db_path = unique_test_db_path();
    let db = Database::open(&db_path).unwrap();

    let session_1 = "sess_del_1";
    let session_2 = "sess_del_2";

    db.init_crawl(&CrawlSessionInit {
        session_id: session_1.into(),
        target_url: "https://site1.com".into(),
        max_pages: 10,
        max_depth: 2,
        respect_robots: true,
        render_js: false,
    })
    .unwrap();

    db.init_crawl(&CrawlSessionInit {
        session_id: session_2.into(),
        target_url: "https://site2.com".into(),
        max_pages: 10,
        max_depth: 2,
        respect_robots: true,
        render_js: false,
    })
    .unwrap();

    // Insert page and issue for session 1
    let mut page = mock_page_report(session_1, "https://site1.com/p1", 200);
    let issue = IssueFinding {
        code: RuleId::ErrHttp4xxClientError,
        category: IssueCategory::Indexability,
        severity: Severity::Critical,
        title: "Broken Page".into(),
        message: "Not found".into(),
        target_url: "https://site1.com/p1".into(),
        source_page_url: None,
    };
    page.issues.push(issue.clone());

    db.save_page_batch(session_1, &[page]).unwrap();
    db.save_issue_batch(session_1, &[issue]).unwrap();

    // Verify session 1 exists
    assert!(db.get_crawl(session_1).unwrap().is_some());
    assert_eq!(db.get_crawl_pages(session_1, 10, 0).unwrap().len(), 1);
    assert_eq!(db.get_crawl_issues(session_1, None, None).unwrap().len(), 2);

    // Delete session 1
    let deleted = db.delete_crawl(session_1).unwrap();
    assert!(deleted, "Expected delete_crawl to return true");

    // Verify session 1 is gone and cascading deletion cleaned up pages and issues
    assert!(db.get_crawl(session_1).unwrap().is_none());
    assert_eq!(db.get_crawl_pages(session_1, 10, 0).unwrap().len(), 0);
    assert_eq!(db.get_crawl_issues(session_1, None, None).unwrap().len(), 0);

    // Session 2 should remain intact
    assert!(db.get_crawl(session_2).unwrap().is_some());

    // Clean all
    let cleaned = db.clean_all_crawls().unwrap();
    assert_eq!(cleaned, 1);
    assert!(db.get_crawl(session_2).unwrap().is_none());

    let _ = std::fs::remove_file(&db_path);
}

#[test]
fn test_storage_query_issues_advanced_filtering() {
    let db_path = unique_test_db_path();
    let db = Database::open(&db_path).unwrap();

    let session = "sess_adv_filter";
    db.init_crawl(&CrawlSessionInit {
        session_id: session.into(),
        target_url: "https://testfilter.com".into(),
        max_pages: 10,
        max_depth: 2,
        respect_robots: true,
        render_js: false,
    })
    .unwrap();

    let issues = vec![
        IssueFinding {
            code: RuleId::ErrHttp4xxClientError,
            category: IssueCategory::Indexability,
            severity: Severity::Critical,
            title: "404 Error".into(),
            message: "Target is 404".into(),
            target_url: "https://testfilter.com/broken".into(),
            source_page_url: None,
        },
        IssueFinding {
            code: RuleId::WarnTitleTooLong,
            category: IssueCategory::TitleMetadata,
            severity: Severity::Warning,
            title: "Title Long".into(),
            message: "Title length > 60".into(),
            target_url: "https://testfilter.com/blog/long-title".into(),
            source_page_url: None,
        },
        IssueFinding {
            code: RuleId::WarnGraphRedirectChain,
            category: IssueCategory::Links,
            severity: Severity::Alert,
            title: "Redirect Chain".into(),
            message: "3 redirect hops".into(),
            target_url: "https://testfilter.com/blog/redirect".into(),
            source_page_url: None,
        },
    ];

    db.save_issue_batch(session, &issues).unwrap();

    // 1. Filter by severity Critical
    let criticals = db
        .query_issues_filtered(
            session,
            &IssueFilterCriteria {
                severity: Some(Severity::Critical),
                limit: 10,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(criticals.len(), 1);
    assert_eq!(criticals[0].code, RuleId::ErrHttp4xxClientError);

    // 2. Filter by category Titles
    let title_issues = db
        .query_issues_filtered(
            session,
            &IssueFilterCriteria {
                category: Some(IssueCategory::TitleMetadata),
                limit: 10,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(title_issues.len(), 1);
    assert_eq!(title_issues[0].code, RuleId::WarnTitleTooLong);

    // 3. Filter by URL pattern '/blog/'
    let blog_issues = db
        .query_issues_filtered(
            session,
            &IssueFilterCriteria {
                url_substring: Some("/blog/"),
                limit: 10,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(blog_issues.len(), 2);

    // 4. Filter by rule code
    let code_issues = db
        .query_issues_filtered(
            session,
            &IssueFilterCriteria {
                code: Some("WARN_GRAPH_REDIRECT_CHAIN"),
                limit: 10,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(code_issues.len(), 1);
    assert_eq!(
        code_issues[0].target_url,
        "https://testfilter.com/blog/redirect"
    );

    let _ = std::fs::remove_file(&db_path);
}

#[tokio::test]
async fn test_crawl_url_include_and_exclude_filters() {
    let server = MockServer::start().await;
    let base = server.uri();

    // Root links to 4 URLs:
    // - /blog/article-1 (included)
    // - /blog/drafts/test (excluded by drafts rule)
    // - /shop/item-1 (excluded by include pattern requiring /blog/)
    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(ResponseTemplate::new(200).set_body_string(format!(
            r#"<!DOCTYPE html><html><head><title>Home</title></head>
            <body>
                <a href="{base}/blog/article-1">Article 1</a>
                <a href="{base}/blog/drafts/test">Draft</a>
                <a href="{base}/shop/item-1">Item 1</a>
            </body></html>"#
        )))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/blog/article-1"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "<!DOCTYPE html><html><head><title>Article 1</title></head><body><p>Article 1 text</p></body></html>",
        ))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/blog/drafts/test"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "<!DOCTYPE html><html><head><title>Draft</title></head><body><p>Draft</p></body></html>",
        ))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/shop/item-1"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "<!DOCTYPE html><html><head><title>Shop</title></head><body><p>Shop</p></body></html>",
        ))
        .mount(&server)
        .await;

    let mut config = CrawlConfig::new(&base).unwrap();
    config.include_regex = Some(r".*/blog/.*".to_string());
    config.exclude_regex = Some(r".*/drafts/.*".to_string());
    config.max_pages = 20;

    let result = run_crawl(&config, None).await.unwrap();

    let crawled_urls: Vec<String> = result.pages.into_iter().map(|p| p.url).collect();

    // Root is crawled
    assert!(crawled_urls.iter().any(|u| u.ends_with('/')));
    // /blog/article-1 matches include and does not match exclude -> crawled
    assert!(crawled_urls.iter().any(|u| u.contains("/blog/article-1")));
    // /blog/drafts/test matches exclude -> MUST NOT be crawled
    assert!(!crawled_urls.iter().any(|u| u.contains("/blog/drafts/test")));
    // /shop/item-1 does not match include -> MUST NOT be crawled
    assert!(!crawled_urls.iter().any(|u| u.contains("/shop/item-1")));
}

#[tokio::test]
async fn test_ai_readiness_audit_live_mock() {
    let server = MockServer::start().await;
    let base = server.uri();

    // robots.txt disallows GPTBot and PerplexityBot, allows others
    Mock::given(method("GET"))
        .and(path("/robots.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "User-agent: GPTBot\nDisallow: /\n\nUser-agent: PerplexityBot\nDisallow: /\n\nUser-agent: *\nAllow: /\n",
        ))
        .mount(&server)
        .await;

    // llms.txt is present and structured
    Mock::given(method("GET"))
        .and(path("/llms.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string(
            "# Acme Corp\n> Fast cloud platform for developers\n\n## Docs\n- [API](https://example.com/api): Core REST API\n",
        ))
        .mount(&server)
        .await;

    // llms-full.txt returns 404
    Mock::given(method("GET"))
        .and(path("/llms-full.txt"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;

    let report = audit_ai_readiness(&base, "SEOLens/1.0", std::time::Duration::from_secs(5))
        .await
        .unwrap();

    assert!(report.llms_txt_found);
    assert!(!report.llms_full_txt_found);
    assert_eq!(report.citation_search_risk, AiSearchRisk::High);
    assert_eq!(
        report
            .retrieval_bots
            .get("PerplexityBot")
            .map(|s| s.as_str()),
        Some("DISALLOWED")
    );
    assert_eq!(
        report
            .retrieval_bots
            .get("OAI-SearchBot")
            .map(|s| s.as_str()),
        Some("ALLOWED")
    );
    assert_eq!(
        report.training_bots.get("GPTBot").map(|s| s.as_str()),
        Some("DISALLOWED")
    );
    assert!(!report.recommendations.is_empty());
}

#[test]
fn test_schema_validator_google_rich_results() {
    // 1. Valid Product Schema with offers
    let valid_product = r#"{
        "@context": "https://schema.org",
        "@type": "Product",
        "name": "Mechanical Keyboard",
        "image": "https://example.com/keyboard.jpg",
        "offers": {
            "@type": "Offer",
            "price": "149.00",
            "priceCurrency": "USD",
            "availability": "https://schema.org/InStock"
        }
    }"#;

    let outcome = validate_raw_schema(valid_product, Some("Product")).unwrap();
    assert!(outcome.is_valid_json);
    assert_eq!(outcome.detected_type.as_deref(), Some("Product"));
    assert!(outcome.is_rich_result_eligible);
    assert!(outcome.missing_required_fields.is_empty());

    // 2. Product Schema missing 'offers'
    let incomplete_product = r#"{
        "@context": "https://schema.org",
        "@type": "Product",
        "name": "Incomplete Keyboard"
    }"#;

    let outcome2 = validate_raw_schema(incomplete_product, Some("Product")).unwrap();
    assert!(outcome2.is_valid_json);
    assert!(!outcome2.is_rich_result_eligible);
    assert!(outcome2
        .missing_required_fields
        .contains(&"offers".to_string()));

    // 3. Non-JSON script preceding valid JSON-LD
    let html_with_js_and_jsonld = r#"
        <html>
        <head>
            <script src="/static/js/bundle.js">console.log("hello");</script>
            <script type="application/ld+json">
            {
                "@context": "https://schema.org",
                "@type": "Product",
                "name": "Headphones",
                "offers": { "price": "99.00" }
            }
            </script>
        </head>
        </html>
    "#;
    let outcome3 = validate_raw_schema(html_with_js_and_jsonld, Some("Product")).unwrap();
    assert!(
        outcome3.is_valid_json,
        "Must find and parse application/ld+json script"
    );
    assert_eq!(outcome3.detected_type.as_deref(), Some("Product"));
    assert!(outcome3.is_rich_result_eligible);

    // 4. Array-valued @type
    let array_type_schema = r#"{
        "@context": "https://schema.org",
        "@type": ["Product", "Item"],
        "name": "Multi-type Product",
        "offers": { "price": "19.99" }
    }"#;
    let outcome4 = validate_raw_schema(array_type_schema, Some("Product")).unwrap();
    assert!(outcome4.is_valid_json);
    assert_eq!(outcome4.detected_type.as_deref(), Some("Product"));
    assert!(outcome4.is_rich_result_eligible);

    // 5. Unsupported @type
    let unsupported_schema = r#"{
        "@context": "https://schema.org",
        "@type": "Thing",
        "name": "Generic Object"
    }"#;
    let outcome5 = validate_raw_schema(unsupported_schema, None).unwrap();
    assert!(outcome5.is_valid_json);
    assert!(
        !outcome5.is_rich_result_eligible,
        "Thing is not eligible for Google Rich Results"
    );
}

#[test]
fn test_cli_local_and_db_path_flags() {
    // 1. Audit command with --local and -L
    let parsed1 =
        Cli::try_parse_from(["seolens", "audit", "https://example.com", "--local"]).unwrap();
    if let Commands::Audit(args) = parsed1.command {
        assert!(args.local);
        assert!(args.db_path.is_none());
    } else {
        panic!("Expected Audit command");
    }

    let parsed2 = Cli::try_parse_from(["seolens", "audit", "https://example.com", "-L"]).unwrap();
    if let Commands::Audit(args) = parsed2.command {
        assert!(args.local);
    } else {
        panic!("Expected Audit command");
    }

    // 2. Audit with explicit --db-path
    let parsed3 = Cli::try_parse_from([
        "seolens",
        "audit",
        "https://example.com",
        "--db-path",
        "/tmp/custom.db",
    ])
    .unwrap();
    if let Commands::Audit(args) = parsed3.command {
        assert_eq!(args.db_path, Some(PathBuf::from("/tmp/custom.db")));
        assert!(!args.local);
    } else {
        panic!("Expected Audit command");
    }

    // 3. List command
    let parsed_list = Cli::try_parse_from(["seolens", "list", "--local"]).unwrap();
    if let Commands::List(args) = parsed_list.command {
        assert!(args.local);
    } else {
        panic!("Expected List command");
    }

    // 4. Mcp command
    let parsed_mcp = Cli::try_parse_from(["seolens", "mcp", "-L"]).unwrap();
    if let Commands::Mcp(args) = parsed_mcp.command {
        assert!(args.local);
    } else {
        panic!("Expected Mcp command");
    }

    // 5. Report command
    let parsed_report = Cli::try_parse_from(["seolens", "report", "crawl_123", "--local"]).unwrap();
    if let Commands::Report(args) = parsed_report.command {
        assert!(args.local);
    } else {
        panic!("Expected Report command");
    }

    // 6. Issues command
    let parsed_issues = Cli::try_parse_from(["seolens", "issues", "crawl_123", "-L"]).unwrap();
    if let Commands::Issues(args) = parsed_issues.command {
        assert!(args.local);
    } else {
        panic!("Expected Issues command");
    }

    // 7. Delete command
    let parsed_del = Cli::try_parse_from(["seolens", "delete", "crawl_123", "--local"]).unwrap();
    if let Commands::Delete(args) = parsed_del.command {
        assert!(args.local);
    } else {
        panic!("Expected Delete command");
    }

    // 8. Clean command
    let parsed_clean = Cli::try_parse_from(["seolens", "clean", "--all", "-L"]).unwrap();
    if let Commands::Clean(args) = parsed_clean.command {
        assert!(args.local);
    } else {
        panic!("Expected Clean command");
    }
}

#[tokio::test]
async fn test_audit_ai_readiness_strips_credentials() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/robots.txt"))
        .respond_with(
            ResponseTemplate::new(200).set_body_string("User-agent: *\nDisallow: /private"),
        )
        .mount(&mock_server)
        .await;
    Mock::given(method("GET"))
        .and(path("/llms.txt"))
        .respond_with(ResponseTemplate::new(200).set_body_string("# AI Documentation"))
        .mount(&mock_server)
        .await;

    let server_url = mock_server.uri();
    let auth_url = server_url.replace("http://", "http://myuser:secretpassword@");

    let report = audit_ai_readiness(&auth_url, "SEOLens/1.0", Duration::from_secs(5))
        .await
        .expect("Audit AI readiness");

    assert!(
        !report.base_url.contains("myuser"),
        "base_url must not contain username"
    );
    assert!(
        !report.base_url.contains("secretpassword"),
        "base_url must not contain password"
    );
}

#[test]
fn test_invalid_include_exclude_regex_validation() {
    let mut config = CrawlConfig::new("https://example.com").unwrap();
    config.include_regex = Some("([unclosed-regex".to_string());
    assert!(
        config.validate().is_err(),
        "Invalid include regex must fail validation"
    );

    config.include_regex = None;
    config.exclude_regex = Some("*invalid-glob-as-regex".to_string());
    assert!(
        config.validate().is_err(),
        "Invalid exclude regex must fail validation"
    );
}
