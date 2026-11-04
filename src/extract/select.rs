//! # Page Selection
//!
//! Chooses which pages of a business website to read after the homepage, by rules on each
//! internal link's text and path (English and Bangla): a contact page first, then an About page,
//! then pages that show what the business sells, then ordering/location pages. Policies, account
//! and cart pages, files, single blog posts and pagination are never chosen. Only links the
//! homepage itself shows on the same site can be chosen, so selection can never reach an
//! address nobody linked.

use super::contacts::percent_decode;
use super::html::PageLink;
use serde::Serialize;
use url::Url;

/// A page chosen for reading, with the rule that chose it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SelectedPage {
    /// The link's URL as the homepage gave it (fragment removed).
    pub url: String,
    /// `contact`, `about`, `catalog`, `locations` or `ordering`.
    pub kind: &'static str,
    /// The link text.
    pub text: String,
}

struct Category {
    kind: &'static str,
    /// Higher is read first.
    priority: u32,
    /// Most pages of this kind chosen.
    limit: usize,
    keywords: &'static [&'static str],
}

const CATEGORIES: &[Category] = &[
    Category {
        kind: "contact",
        priority: 100,
        limit: 1,
        keywords: &[
            "contact",
            "reach us",
            "reach-us",
            "get in touch",
            "find us",
            "find-us",
            "যোগাযোগ",
            "আমাদের ঠিকানা",
        ],
    },
    Category {
        kind: "about",
        priority: 90,
        limit: 1,
        keywords: &[
            "about",
            "our story",
            "our-story",
            "who we are",
            "who-we-are",
            "mission",
            "vision",
            "our values",
            "philosophy",
            "আমাদের সম্পর্কে",
            "আমাদের কথা",
            "লক্ষ্য",
        ],
    },
    Category {
        kind: "catalog",
        priority: 60,
        limit: 2,
        keywords: &[
            "shop",
            "products",
            "product-category",
            "collections",
            "collection",
            "services",
            "service",
            "menu",
            "catalog",
            "catalogue",
            "categories",
            "পণ্য",
            "সেবা",
        ],
    },
    Category {
        kind: "locations",
        priority: 50,
        limit: 1,
        keywords: &[
            "store locator",
            "store-locator",
            "stores",
            "outlet",
            "branches",
            "branch",
            "showroom",
            "locations",
            "শাখা",
            "শোরুম",
        ],
    },
    Category {
        kind: "ordering",
        priority: 40,
        limit: 1,
        keywords: &["how to order", "how-to-order", "order now", "links"],
    },
];

/// Words that mark a link as never worth reading (whole words of its text or path).
const NEVER_WORDS: &[&str] = &[
    "login", "signin", "signup", "register", "account", "accounts", "cart", "carts", "checkout",
    "wishlist", "compare", "search", "track", "tracking", "privacy", "terms", "refund", "refunds",
    "return", "returns", "shipping", "cookie", "cookies", "career", "careers", "jobs", "policy",
    "policies",
];

/// Fragments of a link's text, path or query that mark it as never worth reading.
const NEVER_PATTERNS: &[&str] = &[
    "log-in", "sign-in", "blog/", "news/", "/tag/", "/page/2", "?page=", "lang=", "/feed",
    "wp-admin", "wp-login",
];

/// Path segments that introduce one product; a page below one is a product detail page.
const PRODUCT_DETAIL_SEGMENTS: &[&str] = &["product", "products", "p", "item", "items"];

/// First path segments of listing pages.
const LISTING_ROOTS: &[&str] = &["collections", "product-category", "categories", "category"];

const FILE_EXTENSIONS: &[&str] = &[
    ".jpg", ".jpeg", ".png", ".gif", ".webp", ".svg", ".pdf", ".doc", ".docx", ".xls", ".xlsx",
    ".zip", ".mp4", ".mp3", ".css", ".js", ".xml", ".ico",
];

/// Whether `c` is part of a word; Bangla letters, vowel signs and the virama count.
fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || ('\u{0980}'..='\u{09FF}').contains(&c)
}

/// Lowercase words of `text`.
fn word_tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !is_word_char(c))
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Whether `word` is `keyword` (also with a plural `s`); Bangla keywords also match as a prefix,
/// since suffixes attach to Bangla words ("সেবাসমূহ").
fn word_matches(word: &str, keyword: &str) -> bool {
    if keyword.is_ascii() {
        word == keyword || word.strip_suffix('s') == Some(keyword)
    } else {
        word.starts_with(keyword)
    }
}

/// Whether the words of `keyword` ("reach us", "product-category") appear next to each other, as
/// whole words, among `tokens`.
fn has_keyword(tokens: &[String], keyword: &str) -> bool {
    let wanted = word_tokens(keyword);
    if wanted.is_empty() {
        return false;
    }
    tokens.windows(wanted.len()).any(|window| {
        window
            .iter()
            .zip(&wanted)
            .all(|(word, keyword)| word_matches(word, keyword))
    })
}

/// A page key ignoring scheme, `www.`, fragment and trailing slash.
pub fn page_key(url: &Url) -> String {
    let host = url.host_str().unwrap_or("").to_ascii_lowercase();
    let host = host.trim_start_matches("www.");
    let path = url.path().trim_end_matches('/').to_ascii_lowercase();
    match url.query() {
        Some(query) => format!("{host}{path}?{query}"),
        None => format!("{host}{path}"),
    }
}

/// Whether `url` is on the homepage's site: same host (ignoring `www.`) and port.
pub fn same_site(url: &Url, home: &Url) -> bool {
    let strip = |u: &Url| {
        u.host_str()
            .unwrap_or("")
            .to_ascii_lowercase()
            .trim_start_matches("www.")
            .to_string()
    };
    matches!(url.scheme(), "http" | "https")
        && !strip(url).is_empty()
        && strip(url) == strip(home)
        && url.port_or_known_default() == home.port_or_known_default()
}

/// Whether a path is below a product segment (`/products/<handle>`, `/shop/product/<slug>`).
fn is_product_detail(segments: &[&str]) -> bool {
    segments
        .iter()
        .position(|segment| PRODUCT_DETAIL_SEGMENTS.contains(segment))
        .is_some_and(|at| at + 1 < segments.len())
}

/// Whether a catalog path is a listing: a collection or category root, or a path that ends in a
/// catalog word (`/shop`, `/products`).
fn is_listing(segments: &[&str], keywords: &[&str]) -> bool {
    let last = word_tokens(segments.last().copied().unwrap_or(""));
    segments
        .first()
        .is_some_and(|first| LISTING_ROOTS.contains(first))
        || keywords.iter().any(|keyword| has_keyword(&last, keyword))
}

/// Chooses up to `max_pages` pages from the homepage's links, best first.
pub fn select_pages(home: &Url, links: &[PageLink], max_pages: usize) -> Vec<SelectedPage> {
    if max_pages == 0 {
        return Vec::new();
    }
    let home_key = page_key(home);
    let mut scored: Vec<(u32, usize, usize, usize, &'static str, String, String)> = Vec::new();
    let mut seen: Vec<String> = vec![home_key];
    for (order, link) in links.iter().enumerate() {
        let Ok(mut url) = Url::parse(&link.url) else {
            continue;
        };
        url.set_fragment(None);
        let key = page_key(&url);
        if seen.contains(&key) || !same_site(&url, home) {
            continue;
        }
        let path = percent_decode(url.path()).to_lowercase();
        let query = url.query().map(percent_decode).unwrap_or_default();
        let haystack = format!(
            "{} {}{}{}",
            link.text.to_lowercase(),
            path,
            if query.is_empty() { "" } else { "?" },
            query.to_lowercase()
        );
        let text_words = word_tokens(&link.text);
        let path_words = word_tokens(&format!("{path} {query}"));
        let names =
            |keyword: &&str| has_keyword(&text_words, keyword) || has_keyword(&path_words, keyword);
        if FILE_EXTENSIONS.iter().any(|ext| path.ends_with(ext))
            || NEVER_PATTERNS.iter().any(|part| haystack.contains(part))
            || NEVER_WORDS.iter().any(names)
        {
            continue;
        }
        let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
        let depth = segments.len();
        let Some(category) = CATEGORIES
            .iter()
            .find(|category| category.keywords.iter().any(names))
        else {
            continue;
        };
        // A deep path under a catalog word is usually a single product, not the range, and so is
        // any page below a "product" segment (`/products/<handle>`, `/p/<slug>`).
        if category.kind == "catalog" && (depth > 2 || is_product_detail(&segments)) {
            continue;
        }
        let listing = category.kind != "catalog" || is_listing(&segments, category.keywords);
        seen.push(key);
        scored.push((
            category.priority,
            usize::from(!listing),
            depth,
            order,
            category.kind,
            url.to_string(),
            link.text.clone(),
        ));
    }
    // A filtered listing (`/products?category=x`) teaches less than the listing itself.
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then(a.5.contains('?').cmp(&b.5.contains('?')))
            .then(a.1.cmp(&b.1))
            .then(a.2.cmp(&b.2))
            .then(a.3.cmp(&b.3))
    });
    let mut chosen: Vec<SelectedPage> = Vec::new();
    for (_, _, _, _, kind, url, text) in scored {
        let limit = CATEGORIES
            .iter()
            .find(|category| category.kind == kind)
            .map(|category| category.limit)
            .unwrap_or(1);
        let same_path = |page: &SelectedPage| page.url.split('?').next() == url.split('?').next();
        if chosen.iter().filter(|page| page.kind == kind).count() >= limit
            || chosen.iter().any(same_path)
        {
            continue;
        }
        chosen.push(SelectedPage { url, kind, text });
        if chosen.len() >= max_pages {
            break;
        }
    }
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link(text: &str, url: &str) -> PageLink {
        PageLink {
            text: text.to_string(),
            url: url.to_string(),
        }
    }

    #[test]
    fn prefers_contact_then_about_then_catalog() {
        let home = Url::parse("https://www.shop.test/").unwrap();
        let links = vec![
            link("Home", "https://shop.test/"),
            link("Shop", "https://www.shop.test/shop"),
            link("Cart", "https://www.shop.test/cart"),
            link("আমাদের সম্পর্কে", "https://www.shop.test/about-us#top"),
            link("Privacy Policy", "https://www.shop.test/privacy-policy"),
            link("যোগাযোগ", "https://www.shop.test/contact"),
            link("Facebook", "https://facebook.com/shop"),
            link(
                "Red Saree",
                "https://www.shop.test/shop/sarees/red-saree-123",
            ),
            link("Catalog PDF", "https://www.shop.test/catalog.pdf"),
            link("Outlets", "https://www.shop.test/outlets"),
            link("Contact again", "https://www.shop.test/contact/"),
        ];
        let chosen = select_pages(&home, &links, 5);
        let urls: Vec<&str> = chosen.iter().map(|p| p.url.as_str()).collect();
        assert_eq!(
            urls,
            vec![
                "https://www.shop.test/contact",
                "https://www.shop.test/about-us",
                "https://www.shop.test/shop",
                "https://www.shop.test/outlets",
            ]
        );
        assert_eq!(chosen[0].kind, "contact");
        assert_eq!(select_pages(&home, &links, 2).len(), 2);
        assert!(select_pages(&home, &links, 0).is_empty());
    }

    #[test]
    fn never_leaves_the_site() {
        let home = Url::parse("https://shop.test/").unwrap();
        let links = vec![
            link("Contact", "https://evil.test/contact"),
            link("About", "https://shop.test:8443/about"),
        ];
        assert!(select_pages(&home, &links, 5).is_empty());
    }

    fn urls(home: &str, links: &[(&str, &str)]) -> Vec<String> {
        let home = Url::parse(home).unwrap();
        let links: Vec<PageLink> = links.iter().map(|(t, u)| link(t, u)).collect();
        select_pages(&home, &links, 10)
            .into_iter()
            .map(|page| page.url)
            .collect()
    }

    #[test]
    fn keywords_match_whole_words_not_substrings() {
        let chosen = urls(
            "https://shop.test/",
            &[
                ("Admissions", "https://shop.test/admissions"),
                ("Permission", "https://shop.test/permission-slip"),
                ("Television", "https://shop.test/television"),
                ("Workshop", "https://shop.test/workshop"),
                ("Hotel", "https://shop.test/contactless"),
                ("Accounting", "https://shop.test/accounting"),
                ("Our Mission", "https://shop.test/our-mission"),
                ("Shop", "https://shop.test/the-shop"),
                (
                    "সেবাসমূহ",
                    "https://shop.test/%E0%A6%B8%E0%A7%87%E0%A6%AC%E0%A6%BE",
                ),
            ],
        );
        assert_eq!(
            chosen,
            vec![
                "https://shop.test/our-mission",
                "https://shop.test/the-shop",
                "https://shop.test/%E0%A6%B8%E0%A7%87%E0%A6%AC%E0%A6%BE",
            ]
        );
    }

    #[test]
    fn single_products_are_not_catalog_pages() {
        let chosen = urls(
            "https://shop.test/",
            &[
                ("Red Saree", "https://shop.test/products/red-saree"),
                ("Saree", "https://shop.test/product/saree"),
                ("Saree", "https://shop.test/p/saree-1"),
                ("Saree", "https://shop.test/collections/all/products/x"),
                ("Everything", "https://shop.test/collections/all"),
                ("Products", "https://shop.test/products"),
                ("Cats", "https://shop.test/product-category/sarees"),
            ],
        );
        assert_eq!(
            chosen,
            vec![
                "https://shop.test/products",
                "https://shop.test/collections/all",
            ]
        );
    }
}
