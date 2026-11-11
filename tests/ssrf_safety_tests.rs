//! # SSRF and Egress Protection Test Suite
//!
//! Comprehensive unit and end-to-end tests validating:
//! 1. IP classification (cloud instance metadata, RFC 1918 private subnets, loopback, IPv6 ULA).
//! 2. Port-aware allowlisting and auto-scoping.
//! 3. Pre-flight URL safety validation.
//! 4. End-to-end single page inspection with redirect hop blocking (WireMock).
//! 5. End-to-end crawler execution with redirect hop blocking and allowlist bypass.
//! 6. Strict blocking of cloud instance metadata (169.254.169.254).

use std::net::IpAddr;
use std::time::Duration;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use blacksparrow::core::config::CrawlConfig;
use blacksparrow::core::url::{
    extract_host_and_port_allowlist, is_cloud_metadata_ip, is_private_or_restricted_ip,
    validate_url_safety,
};
use blacksparrow::crawler::engine::run_crawl;
use blacksparrow::crawler::inspector::inspect_url_with_options_ext;
use blacksparrow::SeoResult;

// =========================================================================
// 1. UNIT TESTS: IP CLASSIFICATION & ALLOWLIST SCOPING
// =========================================================================

#[test]
fn test_cloud_metadata_ip_detection() {
    let meta_v4: IpAddr = "169.254.169.254".parse().unwrap();
    let link_local_v4: IpAddr = "169.254.1.1".parse().unwrap();
    let link_local_v6: IpAddr = "fe80::1".parse().unwrap();
    let mapped_meta: IpAddr = "::ffff:169.254.169.254".parse().unwrap();

    assert!(is_cloud_metadata_ip(meta_v4));
    assert!(is_cloud_metadata_ip(link_local_v4));
    assert!(is_cloud_metadata_ip(link_local_v6));
    assert!(is_cloud_metadata_ip(mapped_meta));

    // Standard IPs should not be flagged as cloud metadata
    let loopback: IpAddr = "127.0.0.1".parse().unwrap();
    let rfc1918: IpAddr = "192.168.1.1".parse().unwrap();
    let public_ip: IpAddr = "8.8.8.8".parse().unwrap();

    assert!(!is_cloud_metadata_ip(loopback));
    assert!(!is_cloud_metadata_ip(rfc1918));
    assert!(!is_cloud_metadata_ip(public_ip));
}

#[test]
fn test_private_restricted_ip_detection() {
    // Loopback
    assert!(is_private_or_restricted_ip("127.0.0.1".parse().unwrap()));
    assert!(is_private_or_restricted_ip("127.0.1.1".parse().unwrap()));
    assert!(is_private_or_restricted_ip("::1".parse().unwrap()));

    // RFC 1918 private subnets
    assert!(is_private_or_restricted_ip("10.0.0.1".parse().unwrap()));
    assert!(is_private_or_restricted_ip("172.16.0.1".parse().unwrap()));
    assert!(is_private_or_restricted_ip(
        "172.31.255.255".parse().unwrap()
    ));
    assert!(is_private_or_restricted_ip("192.168.1.1".parse().unwrap()));

    // CGNAT & link-local
    assert!(is_private_or_restricted_ip("100.64.0.1".parse().unwrap()));
    assert!(is_private_or_restricted_ip(
        "169.254.169.254".parse().unwrap()
    ));

    // IPv6 ULA
    assert!(is_private_or_restricted_ip("fc00::1".parse().unwrap()));
    assert!(is_private_or_restricted_ip(
        "fd12:3456:789a::1".parse().unwrap()
    ));

    // Public Internet addresses (must return false)
    assert!(!is_private_or_restricted_ip("8.8.8.8".parse().unwrap()));
    assert!(!is_private_or_restricted_ip("1.1.1.1".parse().unwrap()));
    assert!(!is_private_or_restricted_ip(
        "93.184.216.34".parse().unwrap()
    )); // example.com
}

#[test]
fn test_extract_host_and_port_allowlist() {
    let custom_port = extract_host_and_port_allowlist("http://localhost:3000/api/v1");
    assert_eq!(custom_port, vec!["localhost:3000".to_string()]);

    let ip_with_port = extract_host_and_port_allowlist("http://127.0.0.1:8080/dashboard");
    assert_eq!(ip_with_port, vec!["127.0.0.1:8080".to_string()]);

    let standard_host = extract_host_and_port_allowlist("https://example.com/blog");
    assert_eq!(standard_host, vec!["example.com".to_string()]);

    // Cloud metadata endpoints are NEVER extracted into allowlists
    let meta_v4 = extract_host_and_port_allowlist("http://169.254.169.254/latest/meta-data");
    assert!(meta_v4.is_empty());

    let meta_gcp =
        extract_host_and_port_allowlist("http://metadata.google.internal/computeMetadata/v1");
    assert!(meta_gcp.is_empty());
}

#[tokio::test]
async fn test_validate_url_safety_unit() {
    // 1. Unsupported schemes
    assert!(validate_url_safety("ftp://example.com", false, &[])
        .await
        .is_err());
    assert!(validate_url_safety("file:///etc/passwd", false, &[])
        .await
        .is_err());

    // 2. Cloud metadata is permanently blocked under any config
    assert!(validate_url_safety(
        "http://169.254.169.254/latest/meta-data",
        true,
        &["169.254.169.254".to_string()]
    )
    .await
    .is_err());
    assert!(validate_url_safety(
        "http://metadata.google.internal/computeMetadata",
        true,
        &["metadata.google.internal".to_string()]
    )
    .await
    .is_err());

    // 3. Port-aware allowlist enforcement
    let allowed = vec!["localhost:3000".to_string()];

    // Matches exact port -> OK
    assert!(
        validate_url_safety("http://localhost:3000/page", false, &allowed)
            .await
            .is_ok()
    );

    // Different port on localhost -> Blocked
    assert!(
        validate_url_safety("http://localhost:4040/secret", false, &allowed)
            .await
            .is_err()
    );

    // IP address with exact port match -> OK
    let ip_allowed = vec!["127.0.0.1:8080".to_string()];
    assert!(
        validate_url_safety("http://127.0.0.1:8080/index", false, &ip_allowed)
            .await
            .is_ok()
    );

    // IP address with different port -> Blocked
    assert!(
        validate_url_safety("http://127.0.0.1:9090/admin", false, &ip_allowed)
            .await
            .is_err()
    );
}

// =========================================================================
// 2. END-TO-END TESTS (WIREMOCK)
// =========================================================================

/// E2E Test 1: Full crawler successfully auto-scopes target URL and audits internal pages
#[tokio::test]
async fn test_e2e_crawler_auto_scopes_target_server() -> SeoResult<()> {
    let mock_server = MockServer::start().await;
    let base_uri = mock_server.uri();

    let root_html = r#"<!DOCTYPE html>
<html>
<head><title>Root Page</title></head>
<body>
    <h1>Welcome</h1>
    <a href="/about">About Us</a>
</body>
</html>"#;

    let about_html = r#"<!DOCTYPE html>
<html>
<head><title>About Page</title></head>
<body>
    <h1>About Us</h1>
</body>
</html>"#;

    Mock::given(method("GET"))
        .and(path("/"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(root_html)
                .insert_header("content-type", "text/html; charset=utf-8"),
        )
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/about"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(about_html)
                .insert_header("content-type", "text/html; charset=utf-8"),
        )
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/robots.txt"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&mock_server)
        .await;

    // Notice we do NOT pass any manual allowlist; CrawlConfig::new auto-scopes mock_server.uri()
    let mut config = CrawlConfig::new(&format!("{}/", base_uri))?;
    config.max_pages = 5;
    config.quiet = true;

    let result = run_crawl(&config, None).await?;

    assert_eq!(
        result.pages.len(),
        2,
        "Crawler should successfully crawl both pages on auto-scoped server"
    );
    assert!(result.pages.iter().any(|p| p.url.ends_with('/')));
    assert!(result.pages.iter().any(|p| p.url.ends_with("/about")));

    Ok(())
}

/// E2E Test 2: Redirect hop to an unauthorized private port is strictly blocked
#[tokio::test]
async fn test_e2e_redirect_hop_to_unauthorized_private_port_blocked() -> SeoResult<()> {
    // Server A: The public-facing entry point
    let server_a = MockServer::start().await;
    // Server B: An internal private microservice on a different port
    let server_b = MockServer::start().await;

    // Server A redirects client to Server B's private port
    Mock::given(method("GET"))
        .and(path("/gateway"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", format!("{}/internal-metrics", server_b.uri())),
        )
        .mount(&server_a)
        .await;

    // Server B contains sensitive data that should NEVER be reached
    Mock::given(method("GET"))
        .and(path("/internal-metrics"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(r#"{"status": "secret_internal_data"}"#)
                .insert_header("content-type", "application/json"),
        )
        .mount(&server_b)
        .await;

    // Auto-scope only server_a: allowed_private_hosts is empty, so only server_a is auto-allowed
    let inspect_res = inspect_url_with_options_ext(
        &format!("{}/gateway", server_a.uri()),
        "BlackSparrow/1.0",
        Duration::from_secs(5),
        Vec::new(),
        false,      // allow_all_private_ips = false
        Vec::new(), // no extra allowed hosts
    )
    .await;

    // The inspect call must FAIL because the redirect target port is blocked
    assert!(
        inspect_res.is_err(),
        "Redirect to unauthorized private port must be rejected"
    );
    let err_msg = inspect_res.unwrap_err().to_string();
    assert!(
        err_msg.contains("Blocked: Access to private or restricted IP address"),
        "Error message should clearly report SSRF block: {}",
        err_msg
    );

    // Verify Server B never received any HTTP request
    let server_b_requests = server_b.received_requests().await.unwrap();
    assert!(
        server_b_requests.is_empty(),
        "Server B should never receive requests from blocked redirect hop"
    );

    Ok(())
}

/// E2E Test 3: Redirect hop to a private port succeeds when explicitly whitelisted via allowlist
#[tokio::test]
async fn test_e2e_redirect_hop_permitted_with_explicit_allow_host() -> SeoResult<()> {
    let server_a = MockServer::start().await;
    let server_b = MockServer::start().await;

    // Extract Server B's host:port (e.g. "127.0.0.1:45678")
    let server_b_parsed = url::Url::parse(&server_b.uri()).unwrap();
    let server_b_host_port = format!(
        "{}:{}",
        server_b_parsed.host_str().unwrap(),
        server_b_parsed.port().unwrap()
    );

    Mock::given(method("GET"))
        .and(path("/gateway"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("location", format!("{}/allowed-page", server_b.uri())),
        )
        .mount(&server_a)
        .await;

    Mock::given(method("GET"))
        .and(path("/allowed-page"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(
                    r#"<!DOCTYPE html><html><head><title>Allowed Internal Page</title></head><body><h1>OK</h1></body></html>"#,
                )
                .insert_header("content-type", "text/html; charset=utf-8"),
        )
        .mount(&server_b)
        .await;

    // Explicitly allow Server B's host:port (simulating -a / --allow-host)
    let inspect_res = inspect_url_with_options_ext(
        &format!("{}/gateway", server_a.uri()),
        "BlackSparrow/1.0",
        Duration::from_secs(5),
        Vec::new(),
        false,
        vec![server_b_host_port],
    )
    .await;

    assert!(
        inspect_res.is_ok(),
        "Redirect to Server B must succeed when explicitly allowed"
    );
    let (page, fetch, _) = inspect_res.unwrap();
    assert_eq!(fetch.status_code, 200);
    assert_eq!(page.title.as_deref(), Some("Allowed Internal Page"));
    assert!(fetch.final_url.contains("/allowed-page"));

    Ok(())
}

/// E2E Test 4: Cloud instance metadata (169.254.169.254) is permanently blocked end-to-end
#[tokio::test]
async fn test_e2e_cloud_metadata_permanently_blocked() {
    let result = inspect_url_with_options_ext(
        "http://169.254.169.254/latest/meta-data/iam/security-credentials/",
        "BlackSparrow/1.0",
        Duration::from_secs(2),
        Vec::new(),
        true,                                // Even if user attempts allow_all_private_ips: true
        vec!["169.254.169.254".to_string()], // Even if user tries adding it to allowlist
    )
    .await;

    assert!(
        result.is_err(),
        "Cloud metadata URL must be blocked unconditionally"
    );
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("Access to cloud instance metadata endpoint is permanently prohibited"),
        "Unexpected error message: {}",
        err_msg
    );
}
