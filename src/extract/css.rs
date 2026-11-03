//! # Stylesheet Brand Analysis
//!
//! Reads which font families a site applies to which kinds of text, the weights it declares, and
//! every colour literal its stylesheets write, counted per colour with the custom property or
//! sheet it came from as evidence. A stylesheet is untrusted text: it is scanned with a small
//! tolerant tokenizer (comments, strings, nested blocks) and never executed or fetched further,
//! except that `@import` URLs are reported for the caller to fetch.

use regex::Regex;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::sync::OnceLock;

/// Most evidence entries kept for one colour; its count still grows.
const MAX_EVIDENCE_PER_COLOR: usize = 5;
/// Most text roles recorded for one font family.
const MAX_ROLES_PER_FONT: usize = 8;
/// Longest font family name kept.
const MAX_FAMILY_CHARS: usize = 120;
/// Deepest nesting of `@media`/`@supports`/`@layer` blocks read.
const MAX_NESTING: usize = 8;

const GENERIC_FAMILIES: &[&str] = &[
    "serif",
    "sans-serif",
    "monospace",
    "cursive",
    "fantasy",
    "system-ui",
    "ui-serif",
    "ui-sans-serif",
    "ui-monospace",
    "ui-rounded",
    "emoji",
    "math",
    "fangsong",
    "inherit",
    "initial",
    "unset",
    "revert",
    "-apple-system",
    "blinkmacsystemfont",
    // Operating-system fonts of the "system font stack"; they say nothing about the brand.
    "segoe ui",
    "segoe ui emoji",
    "segoe ui symbol",
    "apple color emoji",
    "noto color emoji",
    "android emoji",
    "emojisymbols",
    "helvetica neue",
    "sfmono-regular",
    "sf mono",
    "menlo",
    "monaco",
    "consolas",
    "liberation mono",
    "courier new",
    "oxygen",
    "oxygen-sans",
    "ubuntu",
    "cantarell",
    "fira sans",
    "droid sans",
    "noto sans",
];

/// Font families that draw symbols rather than lettering.
pub const DEFAULT_ICON_FONT_PATTERN: &str = "(?i)icon|icomoon|font ?awesome|glyph|material (icons|symbols)|dashicons|eicons|themify|ionicons|feather";

/// Where a colour was seen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ColorEvidence {
    /// `css_variable`, `stylesheet`, `inline_style` or `theme_color_meta`.
    #[serde(rename = "type")]
    pub kind: &'static str,
    /// The custom property name, for `css_variable`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The stylesheet or page URL.
    #[serde(rename = "sourceUrl", skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
}

/// A font family the site applies.
#[derive(Debug, Clone, Default, Serialize)]
pub struct FontUse {
    /// The family name as written.
    pub family: String,
    /// Kinds of text it is applied to: `headings`, `body`, `buttons`, `links`.
    pub selectors: Vec<&'static str>,
    /// Weights declared for it, ascending.
    pub weights: Vec<u16>,
}

/// Fonts and colours of one or more stylesheets.
#[derive(Debug, Clone, Default)]
pub struct CssAnalysis {
    /// Families by lowercase name, in first-seen order.
    fonts: Vec<FontUse>,
    font_index: HashMap<String, usize>,
    /// Occurrences of each uppercase `#RRGGBB`.
    pub colors: HashMap<String, u32>,
    /// Where each colour was seen.
    pub evidence: HashMap<String, Vec<ColorEvidence>>,
    /// URLs the sheets `@import`, as written.
    pub imports: Vec<String>,
}

impl CssAnalysis {
    /// The font records, in first-seen order.
    pub fn fonts(&self) -> &[FontUse] {
        &self.fonts
    }

    /// The record of `family`, created when new.
    pub fn font_mut(&mut self, family: &str) -> &mut FontUse {
        let key = family.to_lowercase();
        let index = match self.font_index.get(&key) {
            Some(index) => *index,
            None => {
                self.fonts.push(FontUse {
                    family: family.to_string(),
                    ..FontUse::default()
                });
                let index = self.fonts.len() - 1;
                self.font_index.insert(key, index);
                index
            }
        };
        &mut self.fonts[index]
    }

    /// Counts one occurrence of a colour and remembers where it came from.
    pub fn add_color(&mut self, hex: &str, evidence: ColorEvidence) {
        *self.colors.entry(hex.to_string()).or_insert(0) += 1;
        let seen = self.evidence.entry(hex.to_string()).or_default();
        if seen.len() < MAX_EVIDENCE_PER_COLOR && !seen.contains(&evidence) {
            seen.push(evidence);
        }
    }

    /// Adds weights to a family.
    pub fn add_weights(&mut self, family: &str, weights: &[u16]) {
        let font = self.font_mut(family);
        for weight in weights {
            if !font.weights.contains(weight) {
                font.weights.push(*weight);
            }
        }
        font.weights.sort_unstable();
    }

    fn add_role(&mut self, family: &str, role: &'static str) {
        let font = self.font_mut(family);
        if !font.selectors.contains(&role) && font.selectors.len() < MAX_ROLES_PER_FONT {
            font.selectors.push(role);
        }
    }

    /// Colours ranked by use, most used first, ties by hex.
    pub fn ranked_colors(&self) -> Vec<(String, u32)> {
        let mut ranked: Vec<(String, u32)> = self
            .colors
            .iter()
            .map(|(hex, count)| (hex.clone(), *count))
            .collect();
        ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        ranked
    }
}

/// Analyses one stylesheet and adds its findings to `analysis`.
///
/// `source_url` is recorded as colour evidence; `inline` marks a `<style>` block.
pub fn analyze_css(
    analysis: &mut CssAnalysis,
    css: &str,
    source_url: &str,
    inline: bool,
    icon_fonts: &Regex,
    class_roles: &ClassRoles,
) {
    let cleaned = strip_comments(css);
    let rules = parse_rules(&cleaned, 0);
    let mut variables: HashMap<String, String> = HashMap::new();
    collect_variables(&rules, &mut variables);
    let ctx = SheetContext {
        source_url,
        inline,
        icon_fonts,
        variables: &variables,
        class_roles,
    };
    read_rules(&rules, analysis, &ctx);
}

/// The kinds of text each class name of the page is applied to, read from the HTML (a class on
/// `<body>` styles body text, a class on an `<h1>` styles headings). Utility-class frameworks
/// (Tailwind and similar) set fonts through such classes rather than element selectors.
pub type ClassRoles = HashMap<String, Vec<&'static str>>;

/// Families (and weights where named) of a Google Fonts stylesheet URL, either generation.
pub fn google_fonts(url: &str) -> BTreeMap<String, Vec<u16>> {
    let mut families: BTreeMap<String, Vec<u16>> = BTreeMap::new();
    let Ok(parsed) = url::Url::parse(url) else {
        return families;
    };
    let host = parsed.host_str().unwrap_or("").to_ascii_lowercase();
    if host != "fonts.googleapis.com" && host != "fonts.gstatic.com" {
        return families;
    }
    for (key, value) in parsed.query_pairs() {
        if key != "family" {
            continue;
        }
        for entry in value.split('|') {
            let (name, spec) = entry.split_once(':').unwrap_or((entry, ""));
            let family = name.replace('+', " ").trim().to_string();
            if family.is_empty() {
                continue;
            }
            let weights = families.entry(family).or_default();
            for weight in find_weights(spec) {
                if !weights.contains(&weight) {
                    weights.push(weight);
                }
            }
            weights.sort_unstable();
        }
    }
    families
}

/// A CSS colour literal (`#RGB`, `#RGBA`, `#RRGGBB`, `#RRGGBBAA`, `rgb()`, `rgba()`) as
/// uppercase `#RRGGBB`; `None` for anything else.
pub fn parse_color(value: &str) -> Option<String> {
    let text = value.trim().to_ascii_lowercase();
    if let Some(digits) = text.strip_prefix('#') {
        if !matches!(digits.len(), 3 | 4 | 6 | 8) || !digits.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return None;
        }
        let full: String = if digits.len() <= 4 {
            digits.chars().take(3).flat_map(|c| [c, c]).collect()
        } else {
            digits.chars().take(6).collect()
        };
        return Some(format!("#{}", full.to_ascii_uppercase()));
    }
    if text.starts_with("hsl") {
        return parse_hsl(&text);
    }
    if text.starts_with("oklch(") {
        return parse_oklch(&text);
    }
    let caps = rgb_pattern().captures(&text)?;
    let mut hex = String::from("#");
    for index in 1..=3 {
        let channel: u16 = caps.get(index)?.as_str().parse().ok()?;
        if channel > 255 {
            return None;
        }
        hex.push_str(&format!("{channel:02X}"));
    }
    Some(hex)
}

/// The numbers of a colour function (`hsl(210 40% 50%)`, `oklch(.63 .24 25 / .5)`), alpha
/// dropped; `%` values come back divided by 100.
fn function_numbers(text: &str) -> Option<Vec<f64>> {
    let inner = text.split_once('(')?.1.strip_suffix(')')?;
    let inner = inner.split('/').next()?;
    inner
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|part| !part.is_empty())
        .take(3)
        .map(|part| {
            let part = part.trim_end_matches("deg");
            match part.strip_suffix('%') {
                Some(number) => number.parse::<f64>().ok().map(|n| n / 100.0),
                None => part.parse::<f64>().ok(),
            }
        })
        .collect()
}

fn to_hex(red: f64, green: f64, blue: f64) -> String {
    let channel = |value: f64| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!(
        "#{:02X}{:02X}{:02X}",
        channel(red),
        channel(green),
        channel(blue)
    )
}

fn parse_hsl(text: &str) -> Option<String> {
    let numbers = function_numbers(text)?;
    let [hue, saturation, lightness] = numbers[..] else {
        return None;
    };
    let hue = hue.rem_euclid(360.0) / 360.0;
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let component = |n: f64| {
        let k = (n + hue * 12.0) % 12.0;
        lightness - chroma / 2.0 * (k - 3.0).min(9.0 - k).clamp(-1.0, 1.0)
    };
    Some(to_hex(component(0.0), component(8.0), component(4.0)))
}

/// OKLCH to sRGB (CSS Color 4): OKLCH -> OKLab -> linear sRGB -> gamma-encoded sRGB.
fn parse_oklch(text: &str) -> Option<String> {
    let numbers = function_numbers(text)?;
    let [lightness, chroma, hue] = numbers[..] else {
        return None;
    };
    let lightness = if lightness > 1.0 {
        lightness / 100.0
    } else {
        lightness
    };
    let radians = hue.to_radians();
    let (a, b) = (chroma * radians.cos(), chroma * radians.sin());
    let l = (lightness + 0.396_337_777_4 * a + 0.215_803_757_3 * b).powi(3);
    let m = (lightness - 0.105_561_345_8 * a - 0.063_854_172_8 * b).powi(3);
    let s = (lightness - 0.089_484_177_5 * a - 1.291_485_548 * b).powi(3);
    let red = 4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s;
    let green = -1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s;
    let blue = -0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701 * s;
    let gamma = |c: f64| {
        if c <= 0.003_130_8 {
            12.92 * c
        } else {
            1.055 * c.powf(1.0 / 2.4) - 0.055
        }
    };
    Some(to_hex(gamma(red), gamma(green), gamma(blue)))
}

fn rgb_pattern() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| {
        Regex::new(r"^rgba?\(\s*(\d{1,3})[\s,]+(\d{1,3})[\s,]+(\d{1,3})\s*(?:[,/][^)]*)?\)$")
            .unwrap_or_else(|_| unreachable_regex())
    })
}

/// A regex that never matches; only reached if a constant pattern failed to compile.
fn unreachable_regex() -> Regex {
    Regex::new("$^").unwrap_or_else(|_| unreachable_regex())
}

/// Weights such as `700`, `bold`, or the bounds of a range such as `300 700`.
fn parse_weights(value: &str) -> Vec<u16> {
    match value.trim().to_ascii_lowercase().as_str() {
        "normal" => vec![400],
        "bold" => vec![700],
        other => find_weights(other),
    }
}

/// Every standalone `100`..`900` in a string.
fn find_weights(text: &str) -> Vec<u16> {
    let bytes = text.as_bytes();
    let mut weights = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index].is_ascii_digit() {
            let start = index;
            while index < bytes.len() && bytes[index].is_ascii_digit() {
                index += 1;
            }
            let run = &text[start..index];
            if run.len() == 3 && run.ends_with("00") && run.as_bytes()[0] != b'0' {
                if let Ok(weight) = run.parse::<u16>() {
                    if !weights.contains(&weight) {
                        weights.push(weight);
                    }
                }
            }
        } else {
            index += 1;
        }
    }
    weights
}

enum Rule {
    Style {
        selector: String,
        declarations: Vec<(String, String)>,
    },
    FontFace(Vec<(String, String)>),
    Import(String),
    Group(Vec<Rule>),
}

fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        match rest[start + 2..].find("*/") {
            Some(end) => rest = &rest[start + 2 + end + 2..],
            None => return out,
        }
    }
    out.push_str(rest);
    out
}

/// Index just past the block that starts at `open` (a `{`), honouring strings and nesting.
fn block_end(bytes: &[u8], open: usize) -> usize {
    let mut depth = 0usize;
    let mut index = open;
    let mut quote: Option<u8> = None;
    while index < bytes.len() {
        let byte = bytes[index];
        match quote {
            Some(q) => {
                if byte == b'\\' {
                    index += 1;
                } else if byte == q {
                    quote = None;
                }
            }
            None => match byte {
                b'\\' => index += 1,
                b'"' | b'\'' => quote = Some(byte),
                b'{' => depth += 1,
                b'}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return index + 1;
                    }
                }
                _ => {}
            },
        }
        index += 1;
    }
    bytes.len()
}

/// Index of the next `{` or `;` outside strings and parentheses, from `start`.
fn next_delimiter(bytes: &[u8], start: usize) -> Option<usize> {
    let mut index = start;
    let mut quote: Option<u8> = None;
    let mut parens = 0usize;
    while index < bytes.len() {
        let byte = bytes[index];
        match quote {
            Some(q) => {
                if byte == b'\\' {
                    index += 1;
                } else if byte == q {
                    quote = None;
                }
            }
            None => match byte {
                b'\\' => index += 1,
                b'"' | b'\'' => quote = Some(byte),
                b'(' => parens += 1,
                b')' => parens = parens.saturating_sub(1),
                b'{' | b';' if parens == 0 => return Some(index),
                b'}' if parens == 0 => return Some(index),
                _ => {}
            },
        }
        index += 1;
    }
    None
}

fn parse_rules(css: &str, depth: usize) -> Vec<Rule> {
    let bytes = css.as_bytes();
    let mut rules = Vec::new();
    let mut index = 0usize;
    while index < bytes.len() {
        let Some(delimiter) = next_delimiter(bytes, index) else {
            break;
        };
        let prelude = css[index..delimiter].trim();
        match bytes[delimiter] {
            b';' => {
                if let Some(import) = prelude
                    .strip_prefix("@import")
                    .or_else(|| prelude.strip_prefix("@IMPORT"))
                {
                    if let Some(url) = import_url(import) {
                        rules.push(Rule::Import(url));
                    }
                }
                index = delimiter + 1;
            }
            b'}' => index = delimiter + 1,
            _ => {
                let end = block_end(bytes, delimiter);
                let inner_end = end.saturating_sub(1).max(delimiter + 1).min(css.len());
                let inner = css.get(delimiter + 1..inner_end).unwrap_or("");
                let lower = prelude.to_ascii_lowercase();
                if lower.starts_with("@font-face") {
                    rules.push(Rule::FontFace(parse_declarations(inner)));
                } else if lower.starts_with("@media")
                    || lower.starts_with("@supports")
                    || lower.starts_with("@layer")
                    || lower.starts_with("@container")
                {
                    if depth < MAX_NESTING {
                        rules.push(Rule::Group(parse_rules(inner, depth + 1)));
                    }
                } else if !lower.starts_with('@') && !prelude.is_empty() {
                    rules.push(Rule::Style {
                        selector: prelude.to_string(),
                        declarations: parse_declarations(inner),
                    });
                }
                index = end;
            }
        }
    }
    rules
}

fn parse_declarations(block: &str) -> Vec<(String, String)> {
    let mut declarations = Vec::new();
    for part in split_top_level(block, b';') {
        if let Some((name, value)) = part.split_once(':') {
            let name = name.trim();
            if name.is_empty() || name.contains('{') {
                continue;
            }
            let value = value.trim().trim_end_matches("!important").trim();
            let key = if name.starts_with("--") {
                name.to_string()
            } else {
                name.to_ascii_lowercase()
            };
            declarations.push((key, value.to_string()));
        }
    }
    declarations
}

/// Splits on `separator` outside strings, parentheses and nested blocks.
fn split_top_level(text: &str, separator: u8) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut quote: Option<u8> = None;
    let mut depth = 0usize;
    let mut index = 0usize;
    while index < bytes.len() {
        let byte = bytes[index];
        match quote {
            Some(q) => {
                if byte == b'\\' {
                    index += 1;
                } else if byte == q {
                    quote = None;
                }
            }
            None => match byte {
                b'\\' => index += 1,
                b'"' | b'\'' => quote = Some(byte),
                b'(' | b'{' | b'[' => depth += 1,
                b')' | b'}' | b']' => depth = depth.saturating_sub(1),
                _ if byte == separator && depth == 0 => {
                    parts.push(&text[start..index]);
                    start = index + 1;
                }
                _ => {}
            },
        }
        index += 1;
    }
    if start < text.len() {
        parts.push(&text[start..]);
    }
    parts
}

fn import_url(prelude: &str) -> Option<String> {
    let text = prelude.trim();
    let inner = if let Some(rest) = text.strip_prefix("url(") {
        rest.split(')').next()?.trim()
    } else {
        text.split_whitespace().next()?
    };
    let url = inner.trim_matches(|c| c == '"' || c == '\'').trim();
    (!url.is_empty()).then(|| url.to_string())
}

fn collect_variables(rules: &[Rule], variables: &mut HashMap<String, String>) {
    for rule in rules {
        match rule {
            Rule::Style { declarations, .. } => {
                for (name, value) in declarations {
                    if name.starts_with("--") {
                        variables
                            .entry(name.clone())
                            .or_insert_with(|| value.clone());
                    }
                }
            }
            Rule::Group(nested) => collect_variables(nested, variables),
            _ => {}
        }
    }
}

struct SheetContext<'a> {
    source_url: &'a str,
    inline: bool,
    icon_fonts: &'a Regex,
    variables: &'a HashMap<String, String>,
    class_roles: &'a ClassRoles,
}

fn read_rules(rules: &[Rule], analysis: &mut CssAnalysis, ctx: &SheetContext<'_>) {
    for rule in rules {
        match rule {
            Rule::Import(url) => {
                analysis.imports.push(url.clone());
                for (family, weights) in google_fonts(url) {
                    if !ctx.icon_fonts.is_match(&family) {
                        analysis.add_weights(&family, &weights);
                    }
                }
            }
            Rule::FontFace(declarations) => {
                let mut family = None;
                let mut weights = Vec::new();
                for (name, value) in declarations {
                    if name == "font-family" {
                        family = primary_family(value, &HashMap::new());
                    } else if name == "font-weight" {
                        weights = parse_weights(value);
                    }
                }
                if let Some(family) = family {
                    if !ctx.icon_fonts.is_match(&family) {
                        analysis.add_weights(&family, &weights);
                    }
                }
            }
            Rule::Group(nested) => read_rules(nested, analysis, ctx),
            Rule::Style {
                selector,
                declarations,
            } => read_style_rule(selector, declarations, analysis, ctx),
        }
    }
}

fn read_style_rule(
    selector: &str,
    declarations: &[(String, String)],
    analysis: &mut CssAnalysis,
    ctx: &SheetContext<'_>,
) {
    let mut roles = selector_roles(selector);
    for class in single_classes(selector) {
        for role in ctx.class_roles.get(&class).into_iter().flatten() {
            if !roles.contains(role) {
                roles.push(role);
            }
        }
    }
    let weight = declarations
        .iter()
        .find(|(name, _)| name == "font-weight")
        .and_then(|(_, value)| parse_weights(value).into_iter().min());
    for (name, value) in declarations {
        if name == "font-family" || name == "font" {
            let family = if name == "font" {
                font_shorthand_family(value, ctx.variables)
            } else {
                primary_family(value, ctx.variables)
            };
            if let Some(family) = family {
                if !ctx.icon_fonts.is_match(&family) {
                    analysis.font_mut(&family);
                    for role in &roles {
                        analysis.add_role(&family, role);
                    }
                    if let (Some(weight), false) = (weight, roles.is_empty()) {
                        analysis.add_weights(&family, &[weight]);
                    }
                }
            }
        }
        let evidence = if name.starts_with("--") {
            ColorEvidence {
                kind: "css_variable",
                name: Some(name.clone()),
                source_url: Some(ctx.source_url.to_string()),
            }
        } else {
            ColorEvidence {
                kind: if ctx.inline {
                    "inline_style"
                } else {
                    "stylesheet"
                },
                name: None,
                source_url: Some(ctx.source_url.to_string()),
            }
        };
        for color in colors_in(value) {
            analysis.add_color(&color, evidence.clone());
        }
    }
}

/// Kinds of text a selector list targets.
pub fn selector_roles(selector_list: &str) -> Vec<&'static str> {
    let mut roles: Vec<&'static str> = Vec::new();
    for selector in selector_list.to_ascii_lowercase().split(',') {
        let compact = selector.trim();
        let (types, classes) = simple_selectors(compact);
        let mut push = |role: &'static str| {
            if !roles.contains(&role) {
                roles.push(role);
            }
        };
        if types
            .iter()
            .any(|t| matches!(t.as_str(), "h1" | "h2" | "h3"))
        {
            push("headings");
        }
        if types
            .iter()
            .any(|t| matches!(t.as_str(), "body" | "html" | "p"))
            || compact == ":root"
            || compact == "*"
        {
            push("body");
        }
        if types.iter().any(|t| t == "button")
            || classes.iter().any(|c| c == "btn" || c == "button")
            || compact.contains("submit")
        {
            push("buttons");
        }
        if types.iter().any(|t| t == "a") {
            push("links");
        }
    }
    roles
}

/// Class names of the selectors in a list that consist of one class only (`.font-sans`,
/// `.btn:hover`), the form utility classes take.
fn single_classes(selector_list: &str) -> Vec<String> {
    selector_list
        .split(',')
        .filter_map(|selector| {
            let compact = selector.trim();
            let rest = compact.strip_prefix('.')?;
            let end = rest
                .find(|c: char| !(c.is_alphanumeric() || c == '-' || c == '_' || c == '\\'))
                .unwrap_or(rest.len());
            let tail = &rest[end..];
            (tail.is_empty() || tail.starts_with(':')).then(|| rest[..end].replace('\\', ""))
        })
        .collect()
}

/// Type selector names and class names of one selector.
fn simple_selectors(selector: &str) -> (Vec<String>, Vec<String>) {
    let mut types = Vec::new();
    let mut classes = Vec::new();
    let chars: Vec<char> = selector.chars().collect();
    let mut index = 0usize;
    let mut at_compound_start = true;
    while index < chars.len() {
        let c = chars[index];
        if c.is_whitespace() || matches!(c, '>' | '+' | '~' | '(') {
            at_compound_start = true;
            index += 1;
            continue;
        }
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
            let start = index;
            while index < chars.len()
                && (chars[index].is_ascii_alphanumeric()
                    || chars[index] == '-'
                    || chars[index] == '_')
            {
                index += 1;
            }
            if at_compound_start {
                types.push(chars[start..index].iter().collect());
            }
            at_compound_start = false;
            continue;
        }
        if c == '.' {
            index += 1;
            let start = index;
            while index < chars.len()
                && (chars[index].is_ascii_alphanumeric()
                    || chars[index] == '-'
                    || chars[index] == '_')
            {
                index += 1;
            }
            classes.push(chars[start..index].iter().collect());
            at_compound_start = false;
            continue;
        }
        at_compound_start = false;
        index += 1;
    }
    (types, classes)
}

/// The first named family of a `font-family` value, resolving one level of `var(--x)`.
pub fn primary_family(value: &str, variables: &HashMap<String, String>) -> Option<String> {
    let resolved = resolve_vars(value, variables);
    for part in split_top_level(&resolved, b',') {
        let name: String = part
            .trim()
            .trim_matches(|c| c == '"' || c == '\'')
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let name = clean_family(&name);
        let lower = name.to_ascii_lowercase();
        if !name.is_empty()
            && !GENERIC_FAMILIES.contains(&lower.as_str())
            && !lower.ends_with(" fallback")
            && !name.starts_with("--")
            && !name.starts_with("var(")
        {
            return Some(name.chars().take(MAX_FAMILY_CHARS).collect());
        }
    }
    None
}

/// The family list of a `font` shorthand (everything after the size), resolved like
/// [`primary_family`].
fn font_shorthand_family(value: &str, variables: &HashMap<String, String>) -> Option<String> {
    let resolved = resolve_vars(value, variables);
    let tokens: Vec<&str> = resolved.split_whitespace().collect();
    let size_at = tokens.iter().position(|token| {
        token
            .chars()
            .next()
            .map(|c| c.is_ascii_digit() || c == '.')
            .unwrap_or(false)
            && token.chars().any(|c| c.is_ascii_alphabetic() || c == '%')
    })?;
    let mut rest = tokens[size_at + 1..].to_vec();
    if rest.first() == Some(&"/") {
        rest.drain(..2.min(rest.len()));
    } else if rest.first().map(|t| t.starts_with('/')).unwrap_or(false) {
        rest.remove(0);
    }
    primary_family(&rest.join(" "), &HashMap::new())
}

/// A family name without the hashing of build tools: Next.js writes `__Inter_d65c78` and
/// `'__Inter_Fallback_d65c78'` for Inter.
fn clean_family(name: &str) -> String {
    let Some(rest) = name.strip_prefix("__") else {
        return name.to_string();
    };
    let base = match rest.rsplit_once('_') {
        Some((head, hash)) if hash.len() >= 5 && hash.chars().all(|c| c.is_ascii_hexdigit()) => {
            head
        }
        _ => rest,
    };
    match base.strip_suffix("_Fallback") {
        Some(family) => format!("{} Fallback", family.replace('_', " ")),
        None => base.replace('_', " "),
    }
}

/// `value` with `var(--x)` references replaced by the sheet's custom properties, up to three
/// levels deep (a variable that names another variable).
fn resolve_vars(value: &str, variables: &HashMap<String, String>) -> String {
    let mut resolved = value.to_string();
    for _ in 0..3 {
        if !resolved.contains("var(") {
            break;
        }
        resolved = resolve_vars_once(&resolved, variables);
    }
    resolved
}

fn resolve_vars_once(value: &str, variables: &HashMap<String, String>) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.find("var(") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 4..];
        let Some(end) = after.find(')') else {
            out.push_str(&rest[start..]);
            return out;
        };
        let inner = &after[..end];
        let (name, fallback) = inner.split_once(',').unwrap_or((inner, ""));
        match variables.get(name.trim()) {
            Some(found) => out.push_str(found),
            None => out.push_str(fallback.trim()),
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Every colour literal in a declaration value, in order, outside strings and `url()`.
pub fn colors_in(value: &str) -> Vec<String> {
    let bytes = value.as_bytes();
    let mut colors = Vec::new();
    let mut index = 0usize;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte == b'"' || byte == b'\'' {
            index += 1;
            while index < bytes.len() && bytes[index] != byte {
                if bytes[index] == b'\\' {
                    index += 1;
                }
                index += 1;
            }
            index += 1;
            continue;
        }
        if byte == b'#' {
            let start = index + 1;
            let mut end = start;
            while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'-') {
                end += 1;
            }
            if let Some(color) = parse_color(&value[index..end]) {
                colors.push(color);
            }
            index = end.max(index + 1);
            continue;
        }
        if byte.is_ascii_alphabetic() && (index == 0 || !is_ident_byte(bytes[index - 1])) {
            let start = index;
            while index < bytes.len() && is_ident_byte(bytes[index]) {
                index += 1;
            }
            let word = value[start..index].to_ascii_lowercase();
            if index < bytes.len() && bytes[index] == b'(' {
                let close = value[index..].find(')').map(|offset| index + offset + 1);
                if word == "url" {
                    index = close.unwrap_or(bytes.len());
                } else if matches!(word.as_str(), "rgb" | "rgba" | "hsl" | "hsla" | "oklch") {
                    if let Some(close) = close {
                        if let Some(color) = parse_color(&value[start..close]) {
                            colors.push(color);
                        }
                        index = close;
                    }
                }
            }
            continue;
        }
        index += 1;
    }
    colors
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn icon() -> Regex {
        Regex::new(DEFAULT_ICON_FONT_PATTERN).unwrap()
    }

    #[test]
    fn parses_colour_literals() {
        assert_eq!(parse_color("#abc").as_deref(), Some("#AABBCC"));
        assert_eq!(parse_color("#AbCdEf80").as_deref(), Some("#ABCDEF"));
        assert_eq!(parse_color("rgb(255, 0, 10)").as_deref(), Some("#FF000A"));
        assert_eq!(parse_color("rgba(1 2 3 / 50%)").as_deref(), Some("#010203"));
        assert_eq!(parse_color("rgb(300,0,0)"), None);
        assert_eq!(parse_color("#abcde"), None);
    }

    #[test]
    fn finds_colours_but_not_in_strings_or_urls() {
        let found = colors_in(
            "linear-gradient(#fff, rgba(0,0,0,.5)) url(img.svg#a1b2c3) \"#123456\" #E11D48",
        );
        assert_eq!(found, vec!["#FFFFFF", "#000000", "#E11D48"]);
    }

    #[test]
    fn reads_fonts_roles_weights_variables_and_imports() {
        let css = r#"
            @import url("https://fonts.googleapis.com/css2?family=Hind+Siliguri:wght@400;700&family=Poppins:wght@300..600");
            /* comment with h1 { color: #000 } */
            :root { --brand: #E11D48; --heading-font: "Playfair Display", serif; }
            @font-face { font-family: 'Playfair Display'; font-weight: 700; src: url(a.woff2); }
            @font-face { font-family: 'FontAwesome'; font-weight: 900; }
            h1, h2 { font-family: var(--heading-font); font-weight: bold; color: var(--brand); }
            body { font: 400 16px/1.5 "Hind Siliguri", sans-serif; background: #fff; }
            .btn-primary, .btn { font-family: Poppins; background-color: #e11d48; }
            @media (max-width: 600px) { a:hover { font-family: Poppins; color: #0F172A; } }
            .fa { font-family: FontAwesome; }
        "#;
        let mut analysis = CssAnalysis::default();
        analyze_css(
            &mut analysis,
            css,
            "https://x.test/s.css",
            false,
            &icon(),
            &ClassRoles::new(),
        );
        let fonts: Vec<(&str, Vec<&str>, Vec<u16>)> = analysis
            .fonts()
            .iter()
            .map(|f| (f.family.as_str(), f.selectors.clone(), f.weights.clone()))
            .collect();
        assert!(fonts.contains(&("Playfair Display", vec!["headings"], vec![700])));
        assert!(fonts.contains(&("Hind Siliguri", vec!["body"], vec![400, 700])));
        assert!(fonts.iter().any(|(f, roles, _)| *f == "Poppins"
            && roles.contains(&"buttons")
            && roles.contains(&"links")));
        assert!(!fonts.iter().any(|(f, _, _)| f.contains("FontAwesome")));
        assert_eq!(analysis.colors.get("#E11D48"), Some(&2));
        assert_eq!(analysis.colors.get("#0F172A"), Some(&1));
        assert!(!analysis.colors.contains_key("#000000"));
        let evidence = &analysis.evidence["#E11D48"];
        assert_eq!(evidence[0].kind, "css_variable");
        assert_eq!(evidence[0].name.as_deref(), Some("--brand"));
        assert_eq!(analysis.imports.len(), 1);
    }

    #[test]
    fn reads_google_fonts_urls_of_both_generations() {
        let old = google_fonts("https://fonts.googleapis.com/css?family=Open+Sans:400,700|Lora");
        assert_eq!(old.get("Open Sans"), Some(&vec![400, 700]));
        assert_eq!(old.get("Lora"), Some(&vec![]));
        let new = google_fonts(
            "https://fonts.googleapis.com/css2?family=Lora:ital,wght@0,400;1,700&family=Inter",
        );
        assert_eq!(new.get("Lora"), Some(&vec![400, 700]));
        assert!(google_fonts("https://example.com/css?family=Lora").is_empty());
    }

    #[test]
    fn survives_malformed_css() {
        let mut analysis = CssAnalysis::default();
        analyze_css(
            &mut analysis,
            "h1 { color: #f00; font-family: \"Unclosed ; } @media { p { color: #0f0",
            "inline",
            true,
            &icon(),
            &ClassRoles::new(),
        );
        assert_eq!(analysis.colors.get("#FF0000"), Some(&1));
    }

    #[test]
    fn reads_tailwind_and_next_font_output() {
        let css = r#"
            .__variable_11f647{--font-overused:"overusedGrotesk","overusedGrotesk Fallback"}
            .font-overused{font-family:var(--font-overused)}
            .md\:w-1\/2{width:50%}
            .bg-brand{background:oklch(63.7% .237 25.331)}
            .text-sky{color:hsl(200 100% 50%)}
            h1{font-family:'__Playfair_Display_9f1a2b','__Playfair_Display_Fallback_9f1a2b'}
        "#;
        let mut roles = ClassRoles::new();
        roles.insert("font-overused".to_string(), vec!["body"]);
        let mut analysis = CssAnalysis::default();
        analyze_css(&mut analysis, css, "inline", true, &icon(), &roles);
        let fonts: Vec<(&str, Vec<&str>)> = analysis
            .fonts()
            .iter()
            .map(|f| (f.family.as_str(), f.selectors.clone()))
            .collect();
        assert!(fonts.contains(&("overusedGrotesk", vec!["body"])));
        assert!(fonts.contains(&("Playfair Display", vec!["headings"])));
        assert!(!fonts.iter().any(|(f, _)| f.contains("Fallback")));
        assert_eq!(analysis.colors.get("#FB2C36"), Some(&1));
        assert_eq!(analysis.colors.get("#00AAFF"), Some(&1));
    }
}
