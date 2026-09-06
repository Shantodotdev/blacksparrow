//! # Internationalization & Hreflang In-Flight Rules
//!
//! Evaluates HTML lang attribute, ISO 639-1 / RFC 5646 language/region codes,
//! cross-domain alternates, missing x-default, and missing self-reference.

use crate::core::models::{HreflangTag, IssueFinding, RuleId};
use crate::rules::catalog::get_rule;

/// Validates whether a language or region code is syntactically valid per ISO 639-1 / RFC 5646.
///
/// Valid patterns:
/// - "x-default"
/// - 2 or 3 lowercase ASCII letters (e.g. "en", "es", "zh", "ast")
/// - 2-3 lowercase letters + '-' + 2 uppercase letters (e.g. "en-US", "en-GB", "pt-BR")
/// - 2-3 lowercase letters + '-' + 3 digits (UN M.49, e.g. "es-419")
/// - 2-3 lowercase letters + '-' + 4 letters script (e.g. "zh-Hant", "zh-Hans")
///
/// Invalid patterns:
/// - "en-UK" (UK is not ISO 3166-1 alpha-2, GB is)
/// - Underscores instead of hyphens ("en_US")
/// - Digits or special characters in language portion
/// - Full words like "english" (> 3 chars for primary language subtag)
pub fn is_valid_hreflang_code(code: &str) -> bool {
    let trimmed = code.trim();
    if trimmed.eq_ignore_ascii_case("x-default") {
        return true;
    }

    let parts: Vec<&str> = trimmed.split('-').collect();
    if parts.is_empty() || parts.len() > 3 {
        return false;
    }

    let lang = parts[0];
    // Language subtag must be 2 or 3 ASCII alphabetic characters
    if lang.len() < 2 || lang.len() > 3 || !lang.chars().all(|c| c.is_ascii_alphabetic()) {
        return false;
    }

    if parts.len() > 1 {
        let subtag = parts[1];
        // Check for common error: "UK" instead of standard "GB"
        if subtag.eq_ignore_ascii_case("UK") {
            return false;
        }

        let is_2_alpha_region =
            subtag.len() == 2 && subtag.chars().all(|c| c.is_ascii_alphabetic());
        let is_3_digit_region = subtag.len() == 3 && subtag.chars().all(|c| c.is_ascii_digit());
        let is_4_alpha_script =
            subtag.len() == 4 && subtag.chars().all(|c| c.is_ascii_alphabetic());

        if !is_2_alpha_region && !is_3_digit_region && !is_4_alpha_script {
            return false;
        }
    }

    if parts.len() == 3 {
        let subtag2 = parts[2];
        let is_2_alpha_region =
            subtag2.len() == 2 && subtag2.chars().all(|c| c.is_ascii_alphabetic());
        let is_3_digit_region = subtag2.len() == 3 && subtag2.chars().all(|c| c.is_ascii_digit());
        if !is_2_alpha_region && !is_3_digit_region {
            return false;
        }
    }

    true
}

/// Evaluates page-level internationalization and hreflang annotations.
pub fn check_international(
    html_lang: Option<&str>,
    hreflangs: &[HreflangTag],
    page_url: &str,
    issues: &mut Vec<IssueFinding>,
) {
    // 1. Missing or empty <html lang="...">
    match html_lang {
        Some(lang) if !lang.trim().is_empty() => {}
        _ => {
            let rule = get_rule(RuleId::WarnHtmlLangMissing);
            issues.push(rule.to_finding(
                page_url,
                Some(
                    "Document root <html> tag lacks a lang attribute or contains empty whitespace.",
                ),
            ));
        }
    }

    if hreflangs.is_empty() {
        return;
    }

    let parsed_page_url = url::Url::parse(page_url).ok();
    let page_host = parsed_page_url.as_ref().and_then(|u| u.host_str());

    let mut has_self_reference = false;
    let mut has_x_default = false;

    for hreflang in hreflangs {
        let code = hreflang.lang_code.as_str();

        if code.eq_ignore_ascii_case("x-default") {
            has_x_default = true;
        }

        // Check self-reference
        if hreflang.target_url == page_url {
            has_self_reference = true;
        }

        // 2. Validate ISO 639-1 / RFC 5646 language/region code
        if !is_valid_hreflang_code(code) {
            let rule = get_rule(RuleId::ErrHreflangInvalidLangCode);
            let msg = format!(
                "Hreflang code \"{}\" is not a valid ISO 639-1 / RFC 5646 language or region code.",
                code
            );
            issues.push(rule.to_finding(page_url, Some(&msg)));
        }

        // 3. Cross-domain hreflang alternate
        if let Ok(target_u) = url::Url::parse(&hreflang.target_url) {
            if let Some(target_host) = target_u.host_str() {
                if let Some(host) = page_host {
                    if target_host != host {
                        let rule = get_rule(RuleId::WarnHreflangCrossDomain);
                        let msg = format!(
                            "Hreflang alternate points to external host \"{}\" (page host is \"{}\").",
                            target_host, host
                        );
                        issues.push(rule.to_finding(page_url, Some(&msg)));
                    }
                }
            }
        }
    }

    // 4. Missing self-reference
    if !has_self_reference {
        let rule = get_rule(RuleId::ErrHreflangMissingSelfReference);
        let msg = format!(
            "Page declares alternate hreflangs but lacks a self-referencing tag matching \"{}\".",
            page_url
        );
        issues.push(rule.to_finding(page_url, Some(&msg)));
    }

    // 5. Missing x-default
    if !has_x_default {
        let rule = get_rule(RuleId::WarnHreflangMissingXDefault);
        issues.push(rule.to_finding(
            page_url,
            Some("Hreflang cluster does not define an 'x-default' fallback page."),
        ));
    }
}
