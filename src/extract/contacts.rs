//! # Contact & Social Link Extraction
//!
//! Pattern-based extraction of a business's contact details from page text and links: phone
//! numbers (Bangladeshi mobiles in Latin or Bangla digits, and every `tel:` link), email
//! addresses (text and `mailto:`), WhatsApp chat links, and the canonical Facebook page and
//! Instagram profile links. Rules only: nothing here guesses, so every value is literally on the
//! page.

use regex::Regex;
use std::sync::OnceLock;
use url::Url;

/// Most values kept per contact kind.
pub const MAX_CONTACTS: usize = 20;

const BANGLA_DIGITS: [char; 10] = ['০', '১', '২', '৩', '৪', '৫', '৬', '৭', '৮', '৯'];
const NOT_EMAIL_ENDINGS: &[&str] = &[".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg", ".avif"];
const FACEBOOK_HOSTS: &[&str] = &[
    "facebook.com",
    "www.facebook.com",
    "m.facebook.com",
    "web.facebook.com",
    "fb.com",
];
const INSTAGRAM_HOSTS: &[&str] = &["instagram.com", "www.instagram.com", "m.instagram.com"];
const FACEBOOK_NOT_PAGES: &[&str] = &[
    "sharer",
    "sharer.php",
    "share",
    "share.php",
    "dialog",
    "plugins",
    "tr",
    "l.php",
    "login",
    "login.php",
    "photo",
    "photo.php",
    "photos",
    "watch",
    "events",
    "groups",
    "hashtag",
    "permalink.php",
    "story.php",
    "help",
    "policies",
    "privacy",
    "legal",
    "ads",
    "business",
    "marketplace",
    "gaming",
    "reel",
    "video.php",
    "home.php",
    "messages",
    "notifications",
    "settings",
    "search",
];
const INSTAGRAM_NOT_PROFILES: &[&str] = &[
    "p", "reel", "reels", "tv", "explore", "stories", "accounts", "direct", "about", "legal",
];
const WHATSAPP_HOSTS: &[&str] = &[
    "wa.me",
    "api.whatsapp.com",
    "web.whatsapp.com",
    "chat.whatsapp.com",
];

fn regex(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).unwrap_or_else(|_| never()))
}

/// A regex that never matches; only reached if a constant pattern failed to compile.
fn never() -> Regex {
    Regex::new("$^").unwrap_or_else(|_| never())
}

fn bd_mobile() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    regex(&CELL, r"(?:\+?880[\s.-]?|0)1[3-9]\d{2}(?:[\s.-]?\d){6}")
}

fn tel_link() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    regex(&CELL, r"(?i)tel:([+\d][\d\s().-]{5,20}\d)")
}

fn email() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    regex(
        &CELL,
        r"[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,24}",
    )
}

fn whatsapp() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    regex(
        &CELL,
        r"(?i)https?://(?:wa\.me/\+?\d{6,15}|(?:api|web)\.whatsapp\.com/send/?\?phone=\+?\d{6,15}|chat\.whatsapp\.com/[A-Za-z0-9]{6,40})",
    )
}

fn social_url() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    regex(
        &CELL,
        r#"(?i)(?:https?:)?//(?:[\w-]+\.)?(?:facebook\.com|fb\.com|instagram\.com)/[^\s"'<>()\[\]{}|\\^`]*"#,
    )
}

/// `text` with Bangla digits replaced by Latin digits.
pub fn latin_digits(text: &str) -> String {
    text.chars()
        .map(|c| match BANGLA_DIGITS.iter().position(|d| *d == c) {
            Some(index) => char::from(b'0' + index as u8),
            None => c,
        })
        .collect()
}

fn push_unique(target: &mut Vec<String>, value: String) {
    if target.len() < MAX_CONTACTS && !target.contains(&value) {
        target.push(value);
    }
}

/// Bangladeshi mobile numbers and `tel:` numbers in text, as written (Bangla digits converted).
pub fn find_phones(text: &str) -> Vec<String> {
    let plain = latin_digits(text);
    let bytes = plain.as_bytes();
    let mut found = Vec::new();
    for m in bd_mobile().find_iter(&plain) {
        let before = m.start().checked_sub(1).map(|i| bytes[i]);
        let after = bytes.get(m.end()).copied();
        let bounded_before = !matches!(before, Some(b) if b.is_ascii_digit() || b == b'+');
        let bounded_after = !matches!(after, Some(b) if b.is_ascii_digit());
        if bounded_before && bounded_after {
            push_unique(&mut found, m.as_str().trim().to_string());
        }
    }
    for caps in tel_link().captures_iter(&plain) {
        if let Some(number) = caps.get(1) {
            push_unique(&mut found, number.as_str().trim().to_string());
        }
    }
    found
}

/// Email addresses in text (including `mailto:` links), lowercased.
pub fn find_emails(text: &str) -> Vec<String> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    for m in email().find_iter(text) {
        let before = m.start().checked_sub(1).map(|i| bytes[i]);
        if matches!(before, Some(b) if b.is_ascii_alphanumeric() || b"._%+-".contains(&b)) {
            continue;
        }
        let value = m.as_str().to_ascii_lowercase();
        if !NOT_EMAIL_ENDINGS.iter().any(|end| value.ends_with(end)) {
            push_unique(&mut found, value);
        }
    }
    found
}

/// WhatsApp chat links in text, as written.
pub fn find_whatsapp(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for m in whatsapp().find_iter(text) {
        push_unique(&mut found, m.as_str().to_string());
    }
    found
}

/// Whether a URL opens a WhatsApp chat.
pub fn is_whatsapp_url(url: &Url) -> bool {
    url.host_str()
        .map(|host| WHATSAPP_HOSTS.contains(&host.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

fn parse_social(raw: &str) -> Option<Url> {
    let candidate = if raw.starts_with("//") {
        format!("https:{raw}")
    } else {
        raw.to_string()
    };
    let url = Url::parse(&candidate).ok()?;
    matches!(url.scheme(), "http" | "https").then_some(url)
}

fn is_name(value: &str, max: usize, extra: &[char]) -> bool {
    !value.is_empty()
        && value.chars().count() <= max
        && value
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || extra.contains(&c))
}

/// The canonical `https://www.facebook.com/<page>` form of a page link, or `None` for anything
/// that is not a page (share buttons, posts, login, tracking pixels).
pub fn facebook_page_url(raw: &str) -> Option<String> {
    let url = parse_social(raw)?;
    let host = url.host_str()?.to_ascii_lowercase();
    if !FACEBOOK_HOSTS.contains(&host.as_str()) {
        return None;
    }
    let segments: Vec<&str> = url.path().split('/').filter(|s| !s.is_empty()).collect();
    let first = *segments.first()?;
    let lower = first.to_ascii_lowercase();
    if lower == "profile.php" {
        let ids: Vec<String> = url
            .query_pairs()
            .filter(|(k, _)| k == "id")
            .map(|(_, v)| v.into_owned())
            .collect();
        if ids.len() == 1 && ids[0].len() <= 25 && ids[0].chars().all(|c| c.is_ascii_digit()) {
            return Some(format!(
                "https://www.facebook.com/profile.php?id={}",
                ids[0]
            ));
        }
        return None;
    }
    if lower == "pages" && segments.len() >= 2 {
        let parts: Vec<&str> = segments[1..segments.len().min(3)].to_vec();
        if parts.iter().all(|p| is_name(p, 100, &['.', '-'])) {
            return Some(format!(
                "https://www.facebook.com/pages/{}",
                parts.join("/")
            ));
        }
        return None;
    }
    if FACEBOOK_NOT_PAGES.contains(&lower.as_str()) || !is_name(first, 100, &['.', '-']) {
        return None;
    }
    Some(format!("https://www.facebook.com/{first}"))
}

/// The canonical `https://www.instagram.com/<name>/` form of a profile link, or `None`.
pub fn instagram_profile_url(raw: &str) -> Option<String> {
    let url = parse_social(raw)?;
    let host = url.host_str()?.to_ascii_lowercase();
    if !INSTAGRAM_HOSTS.contains(&host.as_str()) {
        return None;
    }
    let name = url.path().split('/').find(|s| !s.is_empty())?;
    let ascii_name = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_');
    if INSTAGRAM_NOT_PROFILES.contains(&name.to_ascii_lowercase().as_str())
        || !ascii_name
        || name.len() > 30
    {
        return None;
    }
    Some(format!("https://www.instagram.com/{name}/"))
}

/// Canonical Facebook page and Instagram profile links found in text, in order of use.
pub fn find_social_links(text: &str) -> (Vec<String>, Vec<String>) {
    let mut facebook = Vec::new();
    let mut instagram = Vec::new();
    for m in social_url().find_iter(text) {
        let found = m.as_str().trim_end_matches(['.', ',', ';', ':', '!', '?']);
        if let Some(page) = facebook_page_url(found) {
            push_unique(&mut facebook, page);
        }
        if let Some(profile) = instagram_profile_url(found) {
            push_unique(&mut instagram, profile);
        }
    }
    (facebook, instagram)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_bangladeshi_mobiles_in_both_scripts_and_tel_links() {
        let text = "Call ০১৭১১-২২৩৩৪৪ or +880 1911 223344, order 12345678901234, tel:+88029876543";
        let phones = find_phones(text);
        assert_eq!(
            phones,
            vec!["01711-223344", "+880 1911 223344", "+88029876543"]
        );
    }

    #[test]
    fn finds_emails_but_not_image_names() {
        let emails = find_emails("mailto:Hello@Bakery.com.bd logo@2x.png x@y.c info@shop.test");
        assert_eq!(emails, vec!["hello@bakery.com.bd", "info@shop.test"]);
    }

    #[test]
    fn canonicalizes_social_links() {
        assert_eq!(
            facebook_page_url("https://m.facebook.com/SampleBakery/?ref=bookmarks").as_deref(),
            Some("https://www.facebook.com/SampleBakery")
        );
        assert_eq!(
            facebook_page_url("https://www.facebook.com/profile.php?id=1000123").as_deref(),
            Some("https://www.facebook.com/profile.php?id=1000123")
        );
        assert_eq!(
            facebook_page_url("https://www.facebook.com/sharer/sharer.php?u=x"),
            None
        );
        assert_eq!(facebook_page_url("https://www.facebook.com/tr?id=1"), None);
        assert_eq!(
            instagram_profile_url("https://instagram.com/sample.bakery?hl=en").as_deref(),
            Some("https://www.instagram.com/sample.bakery/")
        );
        assert_eq!(
            instagram_profile_url("https://www.instagram.com/p/ABC123/"),
            None
        );
        let (fb, ig) = find_social_links(
            "<a href=\"https://facebook.com/SampleBakery\">fb</a> //www.instagram.com/sample.bakery/.",
        );
        assert_eq!(fb, vec!["https://www.facebook.com/SampleBakery"]);
        assert_eq!(ig, vec!["https://www.instagram.com/sample.bakery/"]);
    }

    #[test]
    fn finds_whatsapp_links() {
        let found = find_whatsapp(
            "chat https://wa.me/8801711223344 or https://api.whatsapp.com/send?phone=8801711223344",
        );
        assert_eq!(found.len(), 2);
    }
}
