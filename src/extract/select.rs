//! # Page Selection
//!
//! Chooses which pages of a business website to read after the homepage, by rules on each
//! internal link's text and path (English and Bangla): a contact page first, then an About page,
//! then pages that show what the business sells, then ordering/location pages. Policies, account
//! and cart pages, files, single blog posts and pagination are never chosen. Only links the
//! homepage itself shows on the same site can be chosen, so selection can never reach an
//! address nobody linked.

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

const NEVER: &[&str] = &[
    "login", "log-in", "signin", "sign-in", "signup", "register", "account", "cart", "checkout",
    "wishlist", "compare", "search", "track", "privacy", "terms", "refund", "return", "shipping",
    "cookie", "career", "jobs", "blog/", "news/", "/tag/", "/page/2", "?page=", "lang=", "/feed",
    "wp-admin", "wp-login",
];

const FILE_EXTENSIONS: &[&str] = &[
    ".jpg", ".jpeg", ".png", ".gif", ".webp", ".svg", ".pdf", ".doc", ".docx", ".xls", ".xlsx",
    ".zip", ".mp4", ".mp3", ".css", ".js", ".xml", ".ico",
];

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

/// Chooses up to `max_pages` pages from the homepage's links, best first.
pub fn select_pages(home: &Url, links: &[PageLink], max_pages: usize) -> Vec<SelectedPage> {
    if max_pages == 0 {
        return Vec::new();
    }
    let home_key = page_key(home);
    let mut scored: Vec<(u32, usize, usize, &'static str, String, String)> = Vec::new();
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
        let path = url.path().to_ascii_lowercase();
        let haystack = format!(
            "{} {}{}",
            link.text.to_lowercase(),
            path,
            url.query().map(|q| format!("?{q}")).unwrap_or_default()
        );
        if FILE_EXTENSIONS.iter().any(|ext| path.ends_with(ext))
            || NEVER.iter().any(|word| haystack.contains(word))
        {
            continue;
        }
        let depth = path.split('/').filter(|s| !s.is_empty()).count();
        let Some(category) = CATEGORIES
            .iter()
            .find(|category| category.keywords.iter().any(|word| haystack.contains(word)))
        else {
            continue;
        };
        // A deep path under a catalog word is usually a single product, not the range.
        if category.kind == "catalog" && depth > 2 {
            continue;
        }
        seen.push(key);
        scored.push((
            category.priority,
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
            .then(a.4.contains('?').cmp(&b.4.contains('?')))
            .then(a.1.cmp(&b.1))
            .then(a.2.cmp(&b.2))
    });
    let mut chosen: Vec<SelectedPage> = Vec::new();
    for (_, _, _, kind, url, text) in scored {
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
}
