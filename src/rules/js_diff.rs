//! JavaScript-rendered DOM comparison rules.
//!
//! These rules run only after a raw HTTP document and its Chrome-rendered DOM have both
//! been parsed. They deliberately compare parsed SEO signals rather than raw HTML strings.

use crate::core::models::{IssueFinding, RobotsFlags, RuleId};
use crate::parser::ParsedPage;
use crate::rules::catalog::get_rule;

/// Evaluates the six JavaScript SEO diff rules for one rendered document.
pub fn evaluate_js_diff(
    raw: &ParsedPage,
    rendered: &ParsedPage,
    raw_url: &str,
    rendered_url: &str,
    runtime_errors: &[String],
) -> Vec<IssueFinding> {
    let mut issues = Vec::new();

    if raw.canonical_url != rendered.canonical_url {
        issues.push(get_rule(RuleId::ErrJsDiffCanonicalAltered).to_finding(
            raw_url,
            Some("The canonical tag in Chrome's rendered DOM differs from the raw HTTP response."),
        ));
    }

    if !raw.robots_flags.contains(RobotsFlags::NOINDEX)
        && rendered.robots_flags.contains(RobotsFlags::NOINDEX)
    {
        issues.push(get_rule(RuleId::ErrJsDiffNoindexInjected).to_finding(
            raw_url,
            Some(
                "JavaScript added a noindex directive that was absent from the raw HTTP response.",
            ),
        ));
    }

    if raw_url != rendered_url
        && raw.title == rendered.title
        && raw.meta_description == rendered.meta_description
    {
        issues.push(get_rule(RuleId::WarnJsDiffTitleMetaDesync).to_finding(
            raw_url,
            Some("Client-side navigation changed the URL without changing the title or meta description."),
        ));
    }

    let raw_has_editorial_content = raw.word_count >= 30;
    let content_mostly_vanished = rendered.word_count < raw.word_count / 2;
    if raw_has_editorial_content && content_mostly_vanished {
        issues.push(get_rule(RuleId::AlertJsDiffVanishingContent).to_finding(
            raw_url,
            Some("Substantial editorial content in the raw HTTP document disappeared after JavaScript executed."),
        ));
    }

    if raw.links.is_empty() && !rendered.links.is_empty() {
        issues.push(get_rule(RuleId::AlertJsDiffLateRenderedLinks).to_finding(
            raw_url,
            Some("Internal navigation links appear only in the client-rendered DOM."),
        ));
    }

    if !runtime_errors.is_empty() && (rendered.word_count < 10 || content_mostly_vanished) {
        issues.push(get_rule(RuleId::ErrJsDiffHydrationCrash).to_finding(
            raw_url,
            Some("Chrome observed an uncaught JavaScript error and the rendered page has little or no editorial content."),
        ));
    }

    issues
}
