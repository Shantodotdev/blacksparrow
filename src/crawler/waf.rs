//! # WAF & Anti-Bot Fingerprint Detection
//!
//! Heuristic fingerprinting for Web Application Firewalls (Cloudflare, Akamai,
//! DataDome, Imperva, AWS WAF) challenge and captcha screens.
//!
//! ## Problem in Technical SEO Crawling
//!
//! When crawling websites protected by edge security, bot protection screens
//! often return HTTP 200 OK or 403 Forbidden containing an HTML/JavaScript challenge
//! (e.g. Cloudflare Turnstile, Akamai Bot Manager, DataDome captcha).
//!
//! Blindly parsing these challenge screens leads to false positives:
//! - "Document title missing or generic ('Just a moment...')"
//! - "Zero word count / Thin content"
//! - "Missing H1 headlines"
//!
//! When a challenge fingerprint is detected, SEO Lens:
//! 1. Flags `ALERT_WAF_BOT_CHALLENGE` on the URL.
//! 2. Suppresses false-positive content and heading warnings.
//! 3. Advises the auditor to allowlist the crawler IP or supply authentication cookies.

use reqwest::header::HeaderMap;

/// Definition of a WAF signature probe.
pub struct WafProbe {
    /// Human-readable provider name (e.g. "Cloudflare", "Akamai").
    pub provider: &'static str,
    /// Case-insensitive substrings matching known challenge screen HTML or script markers.
    pub signatures: &'static [&'static str],
}

/// Known WAF and bot management provider signatures.
pub const WAF_SIGNATURES: &[WafProbe] = &[
    WafProbe {
        provider: "Cloudflare",
        signatures: &[
            "cf-browser-verification",
            "checking your browser before accessing",
            "cloudflare ray id",
            "/cdn-cgi/challenge-platform/",
            "attention required! | cloudflare",
        ],
    },
    WafProbe {
        provider: "Akamai",
        signatures: &[
            "akamai bot manager",
            "reference&#32;number:",
            "bm-sz=",
            "_abck=",
        ],
    },
    WafProbe {
        provider: "DataDome",
        signatures: &["geo.captcha-delivery.com", "datadome", "dd_cookie_test"],
    },
    WafProbe {
        provider: "Imperva",
        signatures: &["incapsula incident id", "_incap_ses", "visid_incap"],
    },
    WafProbe {
        provider: "AWS WAF",
        signatures: &[
            "captcha.awswaf.com",
            "token.awswaf.com",
            "awswafintegration",
            "gokuprops",
        ],
    },
];

/// Detects whether an HTTP response is a WAF challenge or captcha screen.
///
/// Inspects response headers (such as `cf-ray`, `server`, `x-datadome`) and
/// body content against [`WAF_SIGNATURES`].
///
/// # Arguments
///
/// * `status_code` - HTTP status code of the response.
/// * `headers` - HTTP response headers.
/// * `body` - Response body text.
///
/// # Returns
///
/// Returns `Some("ProviderName")` if a known WAF challenge is matched, or `None`.
///
/// # Examples
///
/// ```rust
/// use blacksparrow::crawler::waf::detect_waf;
/// use reqwest::header::HeaderMap;
///
/// let headers = HeaderMap::new();
/// let body = "<div id=\"cf-browser-verification\">Checking browser...</div>";
/// assert_eq!(detect_waf(403, &headers, body), Some("Cloudflare"));
/// ```
pub fn detect_waf(status_code: u16, headers: &HeaderMap, body: &str) -> Option<&'static str> {
    let lower_body = body.to_lowercase();

    // Check Cloudflare header markers
    if (status_code == 403 || status_code == 503 || status_code == 429)
        && headers.contains_key("cf-ray")
        && (lower_body.contains("cf-browser-verification")
            || lower_body.contains("just a moment")
            || lower_body.contains("challenge-platform"))
    {
        return Some("Cloudflare");
    }

    // Check DataDome header markers
    if headers.contains_key("x-datadome")
        || headers
            .get("set-cookie")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.contains("datadome="))
            .unwrap_or(false)
    {
        return Some("DataDome");
    }

    // AWS WAF names its CAPTCHA and challenge actions in a header, on any status
    if headers.contains_key("x-amzn-waf-action") {
        return Some("AWS WAF");
    }

    // Check body signatures
    for probe in WAF_SIGNATURES {
        for &sig in probe.signatures {
            if lower_body.contains(sig) {
                return Some(probe.provider);
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_cloudflare_body() {
        let headers = HeaderMap::new();
        let body =
            "<html><body>Please wait... /cdn-cgi/challenge-platform/h/b/scripts</body></html>";
        assert_eq!(detect_waf(200, &headers, body), Some("Cloudflare"));
    }

    #[test]
    fn test_detect_akamai_body() {
        let headers = HeaderMap::new();
        let body = "<html><body>Access Denied. Reference&#32;Number: 18.123.456</body></html>";
        assert_eq!(detect_waf(403, &headers, body), Some("Akamai"));
    }

    #[test]
    fn test_detect_aws_waf_header_on_any_status() {
        let mut headers = HeaderMap::new();
        headers.insert("x-amzn-waf-action", "captcha".parse().unwrap());
        assert_eq!(detect_waf(405, &headers, ""), Some("AWS WAF"));
        headers.insert("x-amzn-waf-action", "challenge".parse().unwrap());
        assert_eq!(detect_waf(202, &headers, "<html></html>"), Some("AWS WAF"));
    }

    #[test]
    fn test_detect_aws_waf_captcha_body() {
        let headers = HeaderMap::new();
        let body = "<html><head><title>Let's confirm you are human</title></head><body>\
            <script src=\"https://1234.edge.captcha-sdk.awswaf.com/1234/jsapi.js\"></script>\
            <script>window.gokuProps = {\"key\":\"abc\"};</script>\
            <script>AwsWafIntegration.checkForceRefresh();</script>\
            <p>Solve the CAPTCHA puzzle to continue</p>\
            <script src=\"https://x.token.awswaf.com/x/challenge.js\"></script></body></html>";
        assert_eq!(detect_waf(405, &headers, body), Some("AWS WAF"));
        assert_eq!(
            detect_waf(
                200,
                &headers,
                "<script src=\"https://captcha.awswaf.com/a.js\">"
            ),
            Some("AWS WAF")
        );
    }

    #[test]
    fn test_clean_html_returns_none() {
        let headers = HeaderMap::new();
        let body = "<html><head><title>Welcome to our Store</title></head><body>Product catalog</body></html>";
        assert_eq!(detect_waf(200, &headers, body), None);
    }
}
