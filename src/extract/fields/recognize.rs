//! Typed recognizers: find and normalise prices, dates, phone numbers, emails, GTINs, ISBNs,
//! ratings and quantities in text. Each returns a normalised value or `None`, never a guess.

use regex::Regex;
use std::sync::OnceLock;

fn compiled(cell: &'static OnceLock<Option<Regex>>, pattern: &str) -> Option<&'static Regex> {
    cell.get_or_init(|| Regex::new(pattern).ok()).as_ref()
}

/// A price with its currency when one was written.
#[derive(Debug, Clone, PartialEq)]
pub struct Price {
    /// Amount in major units.
    pub amount: f64,
    /// ISO 4217 code.
    pub currency: Option<String>,
}

const CURRENCY_SYMBOLS: &[(&str, &str)] = &[
    ("R$", "BRL"),
    ("A$", "AUD"),
    ("C$", "CAD"),
    ("NZ$", "NZD"),
    ("HK$", "HKD"),
    ("US$", "USD"),
    ("$", "USD"),
    ("€", "EUR"),
    ("£", "GBP"),
    ("¥", "JPY"),
    ("₹", "INR"),
    ("₩", "KRW"),
    ("₽", "RUB"),
    ("₺", "TRY"),
    ("৳", "BDT"),
    ("₫", "VND"),
    ("₪", "ILS"),
    ("zł", "PLN"),
];

const CURRENCY_CODES: &[&str] = &[
    "USD", "EUR", "GBP", "JPY", "CNY", "INR", "AUD", "CAD", "CHF", "SEK", "NOK", "DKK", "PLN",
    "BRL", "MXN", "KRW", "RUB", "ZAR", "NZD", "SGD", "HKD", "BDT", "TRY", "AED", "SAR", "CZK",
    "HUF", "ILS", "THB", "IDR", "MYR", "PHP", "VND",
];

/// Finds a currency code or symbol in `text`.
pub fn parse_currency(text: &str) -> Option<String> {
    static CODE: OnceLock<Option<Regex>> = OnceLock::new();
    if let Some(re) = compiled(&CODE, r"\b([A-Z]{3})\b") {
        for m in re.captures_iter(text) {
            if let Some(code) = m.get(1).map(|c| c.as_str()) {
                if CURRENCY_CODES.contains(&code) {
                    return Some(code.to_string());
                }
            }
        }
    }
    CURRENCY_SYMBOLS
        .iter()
        .find(|(symbol, _)| text.contains(symbol))
        .map(|(_, code)| (*code).to_string())
}

/// Parses a number written with `,` or `.` as thousands or decimal separator.
pub fn parse_number(text: &str) -> Option<f64> {
    static NUM: OnceLock<Option<Regex>> = OnceLock::new();
    let re = compiled(&NUM, r"-?\d(?:[\d.,'\u{a0} ]*\d)?")?;
    let raw = re.find(text)?.as_str();
    normalise_number(raw)
}

fn normalise_number(raw: &str) -> Option<f64> {
    let cleaned: String = raw
        .chars()
        .filter(|c| !matches!(c, ' ' | '\'' | '\u{a0}'))
        .collect();
    let last_dot = cleaned.rfind('.');
    let last_comma = cleaned.rfind(',');
    let normalised = match (last_dot, last_comma) {
        (Some(d), Some(c)) if c > d => cleaned.replace('.', "").replace(',', "."),
        (Some(_), Some(_)) => cleaned.replace(',', ""),
        (None, Some(c)) => {
            let decimals = cleaned.len() - c - 1;
            if cleaned.matches(',').count() == 1 && decimals != 3 {
                cleaned.replace(',', ".")
            } else {
                cleaned.replace(',', "")
            }
        }
        (Some(_), None) if cleaned.matches('.').count() > 1 => cleaned.replace('.', ""),
        _ => cleaned,
    };
    normalised.parse::<f64>().ok().filter(|v| v.is_finite())
}

/// Parses a price. The currency is optional here; use [`find_price`] to require one.
pub fn parse_price(text: &str) -> Option<Price> {
    let amount = parse_number(text)?;
    Some(Price {
        amount,
        currency: parse_currency(text),
    })
}

/// Finds the first amount in `text` written with a currency symbol or code next to it.
pub fn find_price(text: &str) -> Option<Price> {
    static PRICE: OnceLock<Option<Regex>> = OnceLock::new();
    let re = compiled(
        &PRICE,
        r"(?:(?:[A-Z]{3}|[A-Z]{0,2}\$|[€£¥₹₩₽₺৳₫₪])\s?\d[\d.,]*|\d[\d.,]*\s?(?:[A-Z]{3}\b|[€£¥₹₩₽₺৳₫₪]|zł))",
    )?;
    re.find_iter(text)
        .filter_map(|m| parse_price(m.as_str()))
        .find(|p| p.currency.is_some())
}

const MONTHS: &[&str] = &[
    "january",
    "february",
    "march",
    "april",
    "may",
    "june",
    "july",
    "august",
    "september",
    "october",
    "november",
    "december",
];

/// Month number for a full or three-letter English month name (`sept` too).
fn month_from(name: &str) -> Option<u32> {
    let lower = name.trim_end_matches('.').to_ascii_lowercase();
    MONTHS
        .iter()
        .position(|m| {
            *m == lower
                || (lower.len() == 3 && m.starts_with(&lower))
                || lower == "sept" && *m == "september"
        })
        .map(|i| i as u32 + 1)
}

fn valid_date(y: i32, m: u32, d: u32) -> Option<String> {
    if !(1..=12).contains(&m) || d == 0 || !(1000..=9999).contains(&y) {
        return None;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days = match m {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    (d <= days).then(|| format!("{y:04}-{m:02}-{d:02}"))
}

/// Finds a date in `text` and returns it as `YYYY-MM-DD`. Numeric dates with both parts
/// at most 12 are read month first with `/` and day first with `.` or `-`.
pub fn parse_date(text: &str) -> Option<String> {
    static ISO: OnceLock<Option<Regex>> = OnceLock::new();
    static YMD: OnceLock<Option<Regex>> = OnceLock::new();
    static DMY: OnceLock<Option<Regex>> = OnceLock::new();
    static D_MONTH_Y: OnceLock<Option<Regex>> = OnceLock::new();
    static MONTH_D_Y: OnceLock<Option<Regex>> = OnceLock::new();

    let num = |s: Option<regex::Match<'_>>| s.and_then(|m| m.as_str().parse::<u32>().ok());

    if let Some(c) = compiled(&ISO, r"\b(\d{4})-(\d{1,2})-(\d{1,2})")?.captures(text) {
        return valid_date(num(c.get(1))? as i32, num(c.get(2))?, num(c.get(3))?);
    }
    if let Some(c) = compiled(&YMD, r"\b(\d{4})[/.](\d{1,2})[/.](\d{1,2})\b")?.captures(text) {
        return valid_date(num(c.get(1))? as i32, num(c.get(2))?, num(c.get(3))?);
    }
    if let Some(c) = compiled(&DMY, r"\b(\d{1,2})([/.\-])(\d{1,2})[/.\-](\d{4})\b")?.captures(text)
    {
        let (a, b, y) = (num(c.get(1))?, num(c.get(3))?, num(c.get(4))? as i32);
        let slash = c.get(2).is_some_and(|s| s.as_str() == "/");
        let (m, d) = if a > 12 || (!slash && b <= 12) {
            (b, a)
        } else {
            (a, b)
        };
        return valid_date(y, m, d);
    }
    if let Some(c) = compiled(
        &D_MONTH_Y,
        r"(?i)\b(\d{1,2})(?:st|nd|rd|th)?\s+([a-z]{3,9}\.?),?\s+(\d{4})\b",
    )?
    .captures(text)
    {
        if let Some(m) = c.get(2).and_then(|m| month_from(m.as_str())) {
            return valid_date(num(c.get(3))? as i32, m, num(c.get(1))?);
        }
    }
    if let Some(c) = compiled(
        &MONTH_D_Y,
        r"(?i)\b([a-z]{3,9}\.?)\s+(\d{1,2})(?:st|nd|rd|th)?,?\s+(\d{4})\b",
    )?
    .captures(text)
    {
        if let Some(m) = c.get(1).and_then(|m| month_from(m.as_str())) {
            return valid_date(num(c.get(3))? as i32, m, num(c.get(2))?);
        }
    }
    None
}

/// Finds a phone number (7 to 15 digits) and returns it as digits with a leading `+` when
/// one was written.
pub fn parse_phone(text: &str) -> Option<String> {
    static PHONE: OnceLock<Option<Regex>> = OnceLock::new();
    let re = compiled(&PHONE, r"\+?\(?\d[\d\s().\-]{5,22}\d")?;
    re.find_iter(text).find_map(|m| {
        let raw = m.as_str();
        let digits: String = raw.chars().filter(char::is_ascii_digit).collect();
        if !(7..=15).contains(&digits.len()) || parse_date(raw).is_some() {
            return None;
        }
        Some(if raw.starts_with('+') {
            format!("+{digits}")
        } else {
            digits
        })
    })
}

/// Finds an email address, lowercased.
pub fn parse_email(text: &str) -> Option<String> {
    static EMAIL: OnceLock<Option<Regex>> = OnceLock::new();
    let re = compiled(&EMAIL, r"[A-Za-z0-9._%+\-]+@[A-Za-z0-9.\-]+\.[A-Za-z]{2,}")?;
    re.find(text).map(|m| m.as_str().to_ascii_lowercase())
}

fn gs1_check_ok(digits: &[u32]) -> bool {
    let Some((check, body)) = digits.split_last() else {
        return false;
    };
    // GS1 mod-10: weights 3,1,3,... starting from the digit next to the check digit.
    let sum: u32 = body
        .iter()
        .rev()
        .enumerate()
        .map(|(i, d)| if i % 2 == 0 { d * 3 } else { *d })
        .sum();
    (10 - sum % 10) % 10 == *check
}

fn digits_of(text: &str) -> Vec<u32> {
    text.chars().filter_map(|c| c.to_digit(10)).collect()
}

/// Validates a GTIN-8/12/13/14 (EAN, UPC) by its check digit.
pub fn parse_gtin(text: &str) -> Option<String> {
    if text.chars().any(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let digits = digits_of(text);
    (matches!(digits.len(), 8 | 12 | 13 | 14) && gs1_check_ok(&digits)).then(|| {
        digits
            .iter()
            .map(|d| char::from_digit(*d, 10).unwrap_or('0'))
            .collect()
    })
}

/// Validates an ISBN-10 (mod 11) or ISBN-13 (978/979 prefix, GS1 check digit).
pub fn parse_isbn(text: &str) -> Option<String> {
    let compact: String = text
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == 'X' || *c == 'x')
        .collect::<String>()
        .to_ascii_uppercase();
    match compact.len() {
        13 if compact.starts_with("978") || compact.starts_with("979") => {
            gs1_check_ok(&digits_of(&compact)).then_some(compact)
        }
        10 => {
            let sum: u32 = compact
                .chars()
                .enumerate()
                .map(|(i, c)| {
                    let v = if c == 'X' && i == 9 {
                        10
                    } else {
                        c.to_digit(10).unwrap_or(99)
                    };
                    v * (10 - i as u32)
                })
                .sum();
            (sum.is_multiple_of(11) && !compact[..9].contains('X')).then_some(compact)
        }
        _ => None,
    }
}

/// Parses a rating such as `4.6`, `4.6 out of 5` or `3/5`.
pub fn parse_rating(text: &str) -> Option<f64> {
    static RATING: OnceLock<Option<Regex>> = OnceLock::new();
    let re = compiled(
        &RATING,
        r"(?i)(\d+(?:[.,]\d+)?)(?:\s*(?:/|out\s+of|of)\s*(\d+))?",
    )?;
    let c = re.captures(text)?;
    let value = c.get(1)?.as_str().replace(',', ".").parse::<f64>().ok()?;
    let scale = c
        .get(2)
        .and_then(|s| s.as_str().parse::<f64>().ok())
        .unwrap_or(10.0);
    (value >= 0.0 && value <= scale && scale <= 100.0).then_some(value)
}

/// Parses a number with a unit, such as `1.2 kg` or `40 L`.
pub fn parse_quantity(text: &str) -> Option<(f64, String)> {
    static QTY: OnceLock<Option<Regex>> = OnceLock::new();
    let re = compiled(
        &QTY,
        r"(\d+(?:[.,]\d+)?)\s*([A-Za-z°µ%\u{2033}\u{2032}\x22][A-Za-z0-9²³/]*)",
    )?;
    let c = re.captures(text)?;
    let value = c.get(1)?.as_str().replace(',', ".").parse::<f64>().ok()?;
    Some((value, c.get(2)?.as_str().to_string()))
}

/// Checks that `text` is an absolute http(s) URL, resolving it against `base` when relative.
pub fn parse_url(text: &str, base: Option<&url::Url>) -> Option<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed.contains(char::is_whitespace) {
        return None;
    }
    let parsed = match base {
        Some(base) => base.join(trimmed).ok()?,
        None => url::Url::parse(trimmed).ok()?,
    };
    matches!(parsed.scheme(), "http" | "https").then(|| parsed.to_string())
}
