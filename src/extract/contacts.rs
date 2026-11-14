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
const NOT_EMAIL_ENDINGS: &[&str] = &[
    ".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg", ".avif", ".css", ".js", ".ico", ".woff",
    ".woff2", ".ttf", ".map",
];
const NOT_EMAIL_DOMAINS: &[&str] = &["sentry.io", "wixpress.com", "sentry-next.wixpress.com"];
/// Longest stretch of a line scanned for phone numbers in one piece.
const MAX_LINE_WINDOW: usize = 2000;
/// Most characters allowed between a phone word and the number it announces.
const MAX_WORD_GAP: usize = 30;
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
    "like.php",
    "likebox.php",
    "connect",
    "stories",
    "r.php",
    "recover",
    "recover.php",
    "ajax",
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
    "p",
    "reel",
    "reels",
    "tv",
    "explore",
    "stories",
    "accounts",
    "direct",
    "about",
    "legal",
    "share",
    "web",
    "developer",
    "oauth",
    "challenge",
    "privacy",
    "directory",
    "download",
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

/// A Bangladeshi number in international form (+880 or 880, then 9 to 10 digits): landlines and
/// IP-phone numbers as well as mobiles.
fn bd_international() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    regex(&CELL, r"\+?880[\s.-]?\d(?:[\s.-]?\d){8,9}")
}

/// A run of digits that may be a phone number, with common separators.
fn digit_run() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    regex(&CELL, r"\+?\d[\d\s().-]{3,20}\d")
}

/// Words that announce a phone number on the same or the previous line, matched as whole words.
const PHONE_WORDS: &[&str] = &[
    "hotline",
    "hotlines",
    "call",
    "phone",
    "phones",
    "mobile",
    "mob",
    "cell",
    "tel",
    "telephone",
    "contact",
    "helpline",
    "whatsapp",
    "হটলাইন",
    "ফোন",
    "টেলিফোন",
    "মোবাইল",
    "যোগাযোগ",
    "কল",
];

/// Words that, before "no"/"number", make a digit run an identifier rather than a phone.
const ID_WORDS: &[&str] = &[
    "order",
    "invoice",
    "id",
    "tracking",
    "reference",
    "ref",
    "sku",
    "ticket",
    "booking",
    "consignment",
    "awb",
    "receipt",
];

/// Words directly before a digit run that mark it as an amount or identifier.
const AMOUNT_OR_ID_WORDS: &[&str] = &[
    "tk", "taka", "bdt", "rs", "usd", "order", "invoice", "sku", "id", "tracking", "ticket",
    "orderid",
];

fn tel_link() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    regex(&CELL, r"(?i)\btel:([+%\w().\t -]{4,64})")
}

fn date_or_time_shape() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    regex(
        &CELL,
        r"^(?:\d{4}[-./]\d{1,2}[-./]\d{1,2}|\d{1,2}[-./]\d{1,2}[-./]\d{2,4}|\d{1,2}[.:]\d{2}\s*-\s*\d{1,2}[.:]\d{2}|(?:19|20)\d{2}\s*-\s*(?:19|20)?\d{2,4})$",
    )
}

fn obfuscated_at() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    regex(&CELL, r"(?i)\s*[\[(]\s*at\s*[\])]\s*|%40")
}

fn obfuscated_dot() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    regex(&CELL, r"(?i)\s*[\[(]\s*dot\s*[\])]\s*")
}

fn scaled_image_domain() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    regex(&CELL, r"^\d+(?:\.\d+)?x(?:\.|$)")
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
        r"(?i)https?://(?:(?P<prefix>wa\.me/|(?:api|web)\.whatsapp\.com/send/?\?phone=)(?:\+|%2B)?(?P<number>\d(?:(?:[-. +]|%20)?\d)*)|chat\.whatsapp\.com/[A-Za-z0-9]{6,40})",
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

/// Like [`push_unique`] but ignores letter case, for social links.
fn push_unique_ci(target: &mut Vec<String>, value: String) {
    if target.len() < MAX_CONTACTS && !target.iter().any(|v| v.eq_ignore_ascii_case(&value)) {
        target.push(value);
    }
}

/// A normalised identity for a phone number: digits only, with the Bangladeshi `+880`, `880` and
/// `0` prefixes unified, so `+880 1711-223344` and `01711223344` compare equal.
pub fn phone_key(phone: &str) -> String {
    let digits: String = latin_digits(phone)
        .chars()
        .filter(char::is_ascii_digit)
        .collect();
    match digits.strip_prefix("880") {
        Some(rest) if !rest.is_empty() => format!("0{}", rest.trim_start_matches('0')),
        _ => digits,
    }
}

fn push_unique_phone(target: &mut Vec<String>, value: String) {
    let key = phone_key(&value);
    if target.len() < MAX_CONTACTS && !target.iter().any(|v| phone_key(v) == key) {
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
            push_unique_phone(&mut found, m.as_str().trim().to_string());
        }
    }
    for m in bd_international().find_iter(&plain) {
        let before = m.start().checked_sub(1).map(|i| bytes[i]);
        let after = bytes.get(m.end()).copied();
        if !matches!(before, Some(b) if b.is_ascii_digit())
            && !matches!(after, Some(b) if b.is_ascii_digit())
        {
            push_unique_phone(&mut found, m.as_str().trim().to_string());
        }
    }
    for caps in tel_link().captures_iter(&plain) {
        if let Some(number) = caps.get(1).and_then(|raw| parse_tel(raw.as_str())) {
            push_unique_phone(&mut found, number);
        }
    }
    for number in announced_numbers(&plain) {
        push_unique_phone(&mut found, number);
    }
    found
}

/// Percent-decodes `%XX` escapes; malformed escapes are kept as written.
pub(crate) fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        let hex = bytes
            .get(index + 1..index + 3)
            .and_then(|pair| std::str::from_utf8(pair).ok())
            .and_then(|pair| u8::from_str_radix(pair, 16).ok());
        match (bytes[index], hex) {
            (b'%', Some(byte)) => {
                out.push(byte);
                index += 3;
            }
            (byte, _) => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The phone number at the start of a `tel:` target: percent-decoded, and cut after the first
/// complete number so trailing address text is not swallowed.
fn parse_tel(raw: &str) -> Option<String> {
    let decoded = percent_decode(raw);
    let mut out = String::new();
    let mut separator = String::new();
    let mut digits = 0usize;
    for (index, c) in decoded.trim().chars().enumerate() {
        if c.is_ascii_digit() {
            let complete_at = if out.trim_start_matches('+').starts_with("880") {
                13
            } else {
                11
            };
            if digits >= complete_at && !separator.is_empty() {
                break;
            }
            out.push_str(&separator);
            separator.clear();
            out.push(c);
            digits += 1;
        } else if c == '+' && index == 0 {
            out.push(c);
        } else if matches!(c, ' ' | '\t' | '(' | ')' | '.' | '-') {
            separator.push(c);
        } else {
            break;
        }
    }
    (5..=15).contains(&digits).then(|| out.trim().to_string())
}

/// Whether `c` continues a word; Bangla letters and vowel signs count, the visarga (used as a
/// colon) does not.
fn is_word_char(c: char) -> bool {
    c != '\u{0983}' && (c == '_' || c.is_alphanumeric() || ('\u{0980}'..='\u{09FF}').contains(&c))
}

/// Byte ranges of whole-word phone words in a lowercased line.
fn phone_word_spans(lower: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    for word in PHONE_WORDS {
        for (start, _) in lower.match_indices(word) {
            let end = start + word.len();
            let before = lower[..start].chars().next_back();
            let after = lower[end..].chars().next();
            if !before.is_some_and(is_word_char) && !after.is_some_and(is_word_char) {
                spans.push((start, end));
            }
        }
    }
    spans
}

/// The consecutive pieces of at most `max` bytes (cut on character boundaries) of `text`.
fn windows(text: &str, max: usize) -> Vec<&str> {
    let mut pieces = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let mut end = (start + max).min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        pieces.push(&text[start..end]);
        start = end;
    }
    pieces
}

/// Whether the text before a digit run marks it as a price, order number or other identifier.
fn is_amount_or_id_context(lead: &str) -> bool {
    let lead = lead.trim_end();
    if matches!(lead.chars().next_back(), Some('৳' | '$' | '€' | '£' | '#')) {
        return true;
    }
    let tail: Vec<&str> = lead
        .rsplit(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .take(2)
        .collect();
    match tail.as_slice() {
        [last, previous, ..] if matches!(*last, "no" | "number" | "num" | "nr") => {
            ID_WORDS.contains(previous)
        }
        [last, ..] => AMOUNT_OR_ID_WORDS.contains(last),
        [] => false,
    }
}

/// Short numbers (hotlines such as 16516, landlines) written after a phone word on the same
/// line, or alone on the line after a short heading that names a phone ("Hotline:", "Contact").
/// Link targets are skipped, so an order number in a URL is never read as a phone, and dates,
/// opening hours and year ranges are never read as numbers.
fn announced_numbers(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut announced = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mut names_phone = false;
        for window in windows(trimmed, MAX_LINE_WINDOW) {
            let visible = tel_link()
                .replace_all(&strip_link_targets(window), "")
                .to_lowercase();
            let words = phone_word_spans(&visible);
            names_phone |= !words.is_empty();
            let only_numbers = announced && is_only_numbers(&visible);
            for m in digit_run().find_iter(&visible) {
                let named_before = words.iter().any(|&(_, end)| {
                    end <= m.start() && visible[end..m.start()].chars().count() <= MAX_WORD_GAP
                });
                if !named_before && !only_numbers {
                    continue;
                }
                if visible[..m.start()].ends_with(['%', '_']) {
                    continue;
                }
                let text = m.as_str().trim();
                let digits = text.chars().filter(char::is_ascii_digit).count();
                let lead_start = visible[..m.start()]
                    .char_indices()
                    .rev()
                    .nth(40)
                    .map_or(0, |(i, _)| i);
                let not_phone = is_amount_or_id_context(&visible[lead_start..m.start()])
                    || date_or_time_shape().is_match(text);
                // A Bangladeshi number has at most 13 digits (+880 and ten more); pages also write
                // 880 before an 11-digit local number that keeps its leading 0.
                let international = text.trim_start_matches('+').starts_with("880");
                if ((5..=13).contains(&digits) || (digits == 14 && international)) && !not_phone {
                    found.push(text.to_string());
                }
            }
        }
        announced = names_phone && trimmed.chars().count() <= 60;
    }
    found
}

/// Whether a line holds nothing but digit runs and punctuation.
fn is_only_numbers(line: &str) -> bool {
    let rest = digit_run().replace_all(line, "");
    digit_run().is_match(line) && !rest.chars().any(char::is_alphanumeric)
}

/// A Markdown line without the `(target)` part of its links.
fn strip_link_targets(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(start) = rest.find("](") {
        out.push_str(&rest[..start + 1]);
        match rest[start..].find(')') {
            Some(end) => rest = &rest[start + end + 1..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// Email addresses in text (including `mailto:` links), lowercased. Plain obfuscations
/// (`name [at] domain [dot] com`, `%40`) are undone; image names, asset paths and error-tracker
/// addresses are not emails.
pub fn find_emails(text: &str) -> Vec<String> {
    let restored = obfuscated_at().replace_all(text, "@");
    let restored = obfuscated_dot().replace_all(&restored, ".");
    let text: &str = &restored;
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    for m in email().find_iter(text) {
        let before = m.start().checked_sub(1).map(|i| bytes[i]);
        if matches!(before, Some(b) if b.is_ascii_alphanumeric() || b"._%+-".contains(&b)) {
            continue;
        }
        let value = m.as_str().to_ascii_lowercase();
        if is_plausible_email(&value) {
            push_unique(&mut found, value);
        }
    }
    found
}

/// Whether a lowercased address looks like a mailbox rather than an asset name or tracker id.
fn is_plausible_email(value: &str) -> bool {
    let Some((local, domain)) = value.rsplit_once('@') else {
        return false;
    };
    let hex_local = local.len() >= 16 && local.chars().all(|c| c.is_ascii_hexdigit());
    !NOT_EMAIL_ENDINGS.iter().any(|end| value.ends_with(end))
        && !NOT_EMAIL_DOMAINS
            .iter()
            .any(|blocked| domain == *blocked || domain.ends_with(&format!(".{blocked}")))
        && !scaled_image_domain().is_match(domain)
        && !hex_local
}

/// WhatsApp chat links in text. A number link keeps its host and path as written, with the number
/// reduced to its digits (`wa.me/88016270-98372` becomes `wa.me/8801627098372`); group links
/// are kept as written.
pub fn find_whatsapp(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for caps in whatsapp().captures_iter(text) {
        let Some(whole) = caps.get(0) else {
            continue;
        };
        let (Some(prefix), Some(number)) = (caps.name("prefix"), caps.name("number")) else {
            push_unique(&mut found, whole.as_str().to_string());
            continue;
        };
        if let Some(digits) = whatsapp_digits(number.as_str()) {
            let head = &text[whole.start()..prefix.end()];
            push_unique(&mut found, format!("{head}{digits}"));
        }
    }
    found
}

/// The digits of a WhatsApp number written with separators, when they make a plausible number
/// (6 to 15 digits). A number followed by a space and more digits that make it too long (a year
/// after a link, say) is read up to that space.
fn whatsapp_digits(number: &str) -> Option<String> {
    let plausible = |value: &str| {
        let digits: String = value.chars().filter(char::is_ascii_digit).collect();
        (6..=15).contains(&digits.len()).then_some(digits)
    };
    let spaced = number.replace("%20", " ");
    let parts: Vec<&str> = spaced.split(' ').collect();
    (1..=parts.len())
        .rev()
        .find_map(|count| plausible(&parts[..count].join(" ")))
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
    let encoded = ['.', '-', '%'];
    match lower.as_str() {
        "pages" if segments.len() >= 2 => {
            let parts: Vec<&str> = segments[1..segments.len().min(3)].to_vec();
            let reserved = ["category", "create", "launch", "browser", "getting_started"];
            if parts.iter().all(|p| is_name(p, 100, &encoded))
                && !reserved.contains(&parts[0].to_ascii_lowercase().as_str())
            {
                return Some(format!(
                    "https://www.facebook.com/pages/{}",
                    parts.join("/")
                ));
            }
            return None;
        }
        "people" => {
            let (name, id) = (segments.get(1)?, segments.get(2)?);
            let numeric =
                !id.is_empty() && id.len() <= 25 && id.chars().all(|c| c.is_ascii_digit());
            return (is_name(name, 200, &encoded) && numeric)
                .then(|| format!("https://www.facebook.com/people/{name}/{id}"));
        }
        "p" => {
            let slug = segments.get(1)?;
            let tail = slug.rsplit('-').next().unwrap_or("");
            let has_id = slug.contains('-')
                && (5..=25).contains(&tail.len())
                && tail.chars().all(|c| c.is_ascii_digit());
            return (is_name(slug, 200, &encoded) && has_id)
                .then(|| format!("https://www.facebook.com/p/{slug}"));
        }
        "pg" => {
            let name = segments.get(1)?;
            let reserved = FACEBOOK_NOT_PAGES.contains(&name.to_ascii_lowercase().as_str());
            return (!reserved && is_name(name, 100, &['.', '-']))
                .then(|| format!("https://www.facebook.com/{name}"));
        }
        _ => {}
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
            push_unique_ci(&mut facebook, page);
        }
        if let Some(profile) = instagram_profile_url(found) {
            push_unique_ci(&mut instagram, profile);
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
    fn finds_hotlines_and_international_landlines_but_not_prices() {
        let text = "##### Contact\n\n16516 / 88096780016516\n\nPrice ৳ 125000\n\nOrder #1234567 shipped\n\nহটলাইন: ০৯৬১২-৩৪৫৬৭৮\n[Track](https://shop.test/orders/123456789)";
        let phones = find_phones(text);
        assert!(phones.contains(&"88096780016516".to_string()), "{phones:?}");
        assert!(phones.contains(&"16516".to_string()), "{phones:?}");
        assert!(phones.contains(&"09612-345678".to_string()), "{phones:?}");
        assert!(!phones
            .iter()
            .any(|p| p.contains("125000") || p.contains("1234567")));
        assert!(!phones.iter().any(|p| p.contains("123456789")));
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
    fn mobile_facebook_links_become_canonical_pages() {
        for raw in [
            "https://m.facebook.com/browniehut",
            "https://m.facebook.com/browniehut/?ref=bookmarks",
            "//m.facebook.com/browniehut",
        ] {
            assert_eq!(
                facebook_page_url(raw).as_deref(),
                Some("https://www.facebook.com/browniehut"),
                "{raw}"
            );
        }
        let (fb, _) = find_social_links("[fb](https://m.facebook.com/browniehut)");
        assert_eq!(fb, vec!["https://www.facebook.com/browniehut"]);
        assert_eq!(
            facebook_page_url("https://www.facebook.com/m.browniehut").as_deref(),
            Some("https://www.facebook.com/m.browniehut")
        );
    }

    #[test]
    fn finds_whatsapp_links() {
        let found = find_whatsapp(
            "chat https://wa.me/8801711223344 or https://api.whatsapp.com/send?phone=8801711223344",
        );
        assert_eq!(found.len(), 2);
    }

    #[test]
    fn whatsapp_numbers_with_separators_are_normalised_to_digits() {
        let found = find_whatsapp(
            "https://wa.me/88016270-98372 https://wa.me/+880 1627 098372 \
             https://api.whatsapp.com/send?phone=%2B880%201627-098.372&text=hi \
             https://web.whatsapp.com/send/?phone=+8801627098372",
        );
        assert_eq!(
            found,
            vec![
                "https://wa.me/8801627098372",
                "https://api.whatsapp.com/send?phone=8801627098372",
                "https://web.whatsapp.com/send/?phone=8801627098372",
            ]
        );
    }

    #[test]
    fn whatsapp_numbers_of_the_wrong_length_and_neighbouring_text_are_handled() {
        assert_eq!(find_whatsapp("https://wa.me/12345"), Vec::<String>::new());
        assert_eq!(
            find_whatsapp("https://wa.me/1234567890123456"),
            Vec::<String>::new()
        );
        assert_eq!(
            find_whatsapp("Chat https://wa.me/8801711223344 2024 or https://wa.me/8801711223344."),
            vec!["https://wa.me/8801711223344"]
        );
        assert_eq!(
            find_whatsapp("https://chat.whatsapp.com/AbCdEf123456"),
            vec!["https://chat.whatsapp.com/AbCdEf123456"]
        );
    }

    #[test]
    fn dates_hours_and_year_ranges_are_not_phones() {
        let text = "Hotel Open 9.00-21.00\nCall: 2024-05-12\nTel 2015-2020\nফোন: ০১-০৫-২০২৪\nCalling 12345678\nকলেজ 12345678\nHotel info 123456\nContact\n2015-2020";
        assert_eq!(find_phones(text), Vec::<String>::new());
    }

    #[test]
    fn phone_words_must_precede_the_number_as_whole_words() {
        let text = "Phone no 0961-2345678\nHotline No 16516\nমোবাইলঃ ০৯৬১২৩৪৫৬৭৯\nOrder no 123456\nvalid 7654321\nInvoice number 654321";
        let phones = find_phones(text);
        assert!(phones.contains(&"0961-2345678".to_string()), "{phones:?}");
        assert!(phones.contains(&"16516".to_string()), "{phones:?}");
        assert!(phones.contains(&"09612345679".to_string()), "{phones:?}");
        assert_eq!(phones.len(), 3, "{phones:?}");
    }

    #[test]
    fn tel_links_are_decoded_and_stop_after_the_number() {
        assert_eq!(
            find_phones("tel:+880%201711%20223344"),
            vec!["+880 1711 223344"]
        );
        assert_eq!(
            find_phones("Tel:01711223344 12 Road 5"),
            vec!["01711223344"]
        );
    }

    #[test]
    fn phones_are_deduplicated_by_number() {
        let phones = find_phones("Call 01711-223344 or +880 1711 223344 or tel:+8801711223344");
        assert_eq!(phones, vec!["01711-223344"]);
        assert_eq!(phone_key("+880 1711-223344"), phone_key("01711223344"));
    }

    #[test]
    fn long_lines_are_scanned_in_linear_time() {
        let line = format!("call {}", "1 ".repeat(180_000));
        let started = std::time::Instant::now();
        let _ = find_phones(&line);
        assert!(started.elapsed().as_secs() < 5);
    }

    #[test]
    fn facebook_people_pages_and_noise_links() {
        let canon = |u: &str| facebook_page_url(u);
        assert_eq!(
            canon("https://www.facebook.com/people/Sample-Cafe/100012345/?ref=x").as_deref(),
            Some("https://www.facebook.com/people/Sample-Cafe/100012345")
        );
        assert_eq!(
            canon("https://facebook.com/p/Sample-Cafe-100012345/").as_deref(),
            Some("https://www.facebook.com/p/Sample-Cafe-100012345")
        );
        assert_eq!(
            canon("https://facebook.com/pg/SampleCafe/posts").as_deref(),
            Some("https://www.facebook.com/SampleCafe")
        );
        for bad in [
            "https://facebook.com/people",
            "https://facebook.com/p/",
            "https://facebook.com/like.php?href=x",
            "https://facebook.com/plugins/likebox.php?href=x",
            "https://facebook.com/connect/x",
            "https://facebook.com/r.php",
            "https://facebook.com/recover/initiate",
            "https://facebook.com/dialog/share",
            "https://facebook.com/stories/1",
        ] {
            assert_eq!(canon(bad), None, "{bad}");
        }
        for bad in [
            "share",
            "web",
            "developer",
            "accounts",
            "explore",
            "p",
            "reel",
            "reels",
            "stories",
            "tv",
        ] {
            assert_eq!(
                instagram_profile_url(&format!("https://instagram.com/{bad}/x")),
                None,
                "{bad}"
            );
        }
        let (fb, ig) = find_social_links(
            "https://facebook.com/SampleCafe https://www.facebook.com/samplecafe https://instagram.com/Sample.Cafe https://instagram.com/sample.cafe",
        );
        assert_eq!((fb.len(), ig.len()), (1, 1));
    }

    #[test]
    fn deobfuscates_emails_and_rejects_asset_lookalikes() {
        let text = "a[at]shop.test b (at) shop.test c [at] shop [dot] test d%40shop.test image@2x.png x@1.5x a@b.css a@b.js e@2x.webp 0123456789abcdef0123456789abcdef@o1.ingest.sentry.io";
        assert_eq!(
            find_emails(text),
            vec!["a@shop.test", "b@shop.test", "c@shop.test", "d@shop.test"]
        );
    }
}
