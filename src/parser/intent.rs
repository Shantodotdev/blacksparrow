//! # Smart Page Intent & Archetype Classifier
//!
//! Evidential multinomial logistic classifier that categorizes web pages into
//! one of six archetypes:
//! - `Product`: E-commerce product detail page (PDP) with transactional buying intent.
//! - `Article`: Editorial news article, longform blog post, or journalistic content.
//! - `Category`: E-commerce category, product listing page (PLP), or catalog index.
//! - `Contact`: Contact page, support form, office location, or reach-us portal.
//! - `Homepage`: Root domain landing page or brand homepage.
//! - `Standard`: Neutral reference baseline (docs, legal, utility, or general pages).
//!
//! ## Algorithmic Architecture
//!
//! Rather than relying on brittle CSS class names (e.g. `.add-to-cart`), language-locked
//! English strings, or circular schema assumptions, the classifier computes a
//! multinomial log-odds distribution across 6 archetypes based on:
//! 1. **Spatial Transactional Micro-Clusters**: Physical character proximity (<= 1,800 chars)
//!    between price patterns, quantity input elements, and transactional CTA buttons.
//! 2. **Multilingual Intent Lexicon**: Transactional and availability vocabularies across
//!    12 languages (English, German, French, Spanish, Italian, Dutch, Russian, Arabic,
//!    Chinese, Japanese, Bengali, Portuguese).
//! 3. **Continuous Narrative Prose Analysis**: Identification of unfragmented editorial text runs
//!    (>= 120 chars) coupled with plain-text author bylines and publication dates.
//! 4. **Catalog Grid Uniformity & Link Density**: High concentration of repeating prices
//!    combined with navigation links and internal pagination controls.
//! 5. **Chrome DevTools Protocol (CDP) Ground Truth**: Optional fusion of runtime JavaScript
//!    globals (`window.ShopifyAnalytics`, `window.dataLayer`) and visual CSSOM geometry
//!    (above-the-fold hero prices, CSS Grid card uniformity) for Client-Side Rendered (CSR) SPAs.

use crate::core::models::{PageArchetype, PageIntent};
use crate::parser::ParsedPage;
use compact_str::CompactString;
use regex::Regex;
use std::collections::HashMap;
use std::sync::LazyLock;

/// All 6 supported archetypes evaluated in the multinomial distribution.
pub const ALL_ARCHETYPES: [PageArchetype; 6] = [
    PageArchetype::Standard,
    PageArchetype::Product,
    PageArchetype::Article,
    PageArchetype::Category,
    PageArchetype::Contact,
    PageArchetype::Homepage,
];

/// Optional Chrome DevTools Protocol runtime and CSSOM visual layout signals.
#[derive(Debug, Clone, Default)]
pub struct CdpIntentSignals {
    /// Whether e-commerce runtime globals indicate a Product Detail Page (PDP).
    pub is_platform_pdp: bool,
    /// Whether runtime globals indicate a Product Listing Page (PLP) / Collection.
    pub is_platform_plp: bool,
    /// Whether runtime globals indicate a Homepage.
    pub is_platform_home: bool,
    /// Whether a prominent price is visually rendered above the fold (top < innerHeight).
    pub hero_price_above_fold: bool,
    /// Computed font size in pixels of the above-the-fold hero price.
    pub hero_price_font_size: f32,
    /// Count of repeating sibling elements with matching rendered widths (CSS Grid / Flexbox).
    pub uniform_card_count: u32,
    /// Post-JavaScript rendered HTML for CSR SPAs.
    pub rendered_html: Option<String>,
}

// ---------------------------------------------------------------------------
// Pre-compiled Regular Expressions (Thread-Safe Lazy Initialization)
// ---------------------------------------------------------------------------

static RE_PRICE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(?:[\$€£৳₹¥₩₽₪₫฿₴₸₺元円]|CHF|USD|EUR|GBP|BDT|CAD|AUD|JPY|CNY|INR|RUB|SAR|AED|EGP|TRY|BRL|PLN)(?:<[^>]+>|\s)*[\d,]*\d[\d.]*|[\d,]*\d[\d.]*(?:<[^>]+>|\s)*(?:[\$€£৳₹¥₩₽₪₫฿₴₸₺元円]|CHF|USD|EUR|GBP|BDT|CAD|AUD|JPY|CNY|INR|RUB|SAR|AED|EGP|TRY|BRL|PLN)"#)
        .expect("RE_PRICE regex compilation failed")
});

static RE_SCRIPT_STYLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<(?:script|style)[^>]*>.*?</(?:script|style)>"#)
        .expect("RE_SCRIPT_STYLE regex compilation failed")
});

static RE_DT_TAG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)<dt\b[^>]*>"#).expect("RE_DT_TAG regex compilation failed"));

static RE_TABLE_ROW: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?is)<tr\b[^>]*>(.*?)</tr>"#).expect("RE_TABLE_ROW regex compilation failed")
});

static RE_TD_TH_TAG: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)</(?:td|th)>"#).expect("RE_TD_TH_TAG regex compilation failed")
});

static RE_QTY_INPUT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)<input[^>]+(?:name=["']?(?:qty|quantity)["']?|type=["']?number["']?)"#)
        .expect("RE_QTY_INPUT regex compilation failed")
});

static RE_CTA_BUTTON: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(?:add\s+to\s+(?:cart|bag)|buy\s+now|order\s+now|purchase|in\s+den\s+warenkorb|in\s+den\s+einkaufswagen|jetzt\s+kaufen|ajouter\s+au\s+panier|acheter|añadir\s+a\s+la\s+cesta|agregar\s+al\s+carrito|comprar\s+ya|aggiungi\s+al\s+carrello|acquista\s+ora|in\s+winkelwagen|bestel\s+nu|в\s+корзину|купить|কার্টে\s+যোগ\s+করুন|এখনই\s+কিনুন|カートに入れる|今すぐ購入|加入购物车|立即购买|أضف\s+إلى\s+السلة|اشتري\s+الآن)"#)
        .expect("RE_CTA_BUTTON regex compilation failed")
});

static RE_AVAILABILITY: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\b(?:in\s+stock|out\s+of\s+stock|auf\s+lager|nicht\s+auf\s+lager|en\s+stock|épuisé|disponible|agotado|disponibile|op\s+voorraad|в\s+наличии|স্টকে\s+আছে|在庫あり|有现货|متوفر)\b"#)
        .expect("RE_AVAILABILITY regex compilation failed")
});

static RE_BYLINE_AUTHOR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(?:by|written\s+by|author|von|par|por|автор|লেখক|著者|作者)\s+([A-Z][a-z]+(?:\s+[A-Z][a-z]+){1,3})"#)
        .expect("RE_BYLINE_AUTHOR regex compilation failed")
});

static RE_DATE_PUBLISHED: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(?:published|updated|posted|veröffentlicht|publié|publicado|опубликовано|প্রকাশিত)\s*(?:on|le|am|el)?\s*(?:[A-Za-z]+ \d{1,2},? \d{4}|\d{4}-\d{2}-\d{2}|\d{1,2}/\d{1,2}/\d{4})"#)
        .expect("RE_DATE_PUBLISHED regex compilation failed")
});

static RE_CONTACT_FORM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)<form[^>]*>[\s\S]{0,2500}?(?:name=["']?(?:email|message|subject|contact)["']?|type=["']?email["']?|<textarea)[\s\S]{0,2500}?</form>"#)
        .expect("RE_CONTACT_FORM regex compilation failed")
});

static RE_PAGINATION: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)rel=["']?(?:next|prev)["']?|class=["'][^"']*(?:pagination|page-numbers|page-link)[^"']*["']|\bpage=\d+|\bp=\d+"#)
        .expect("RE_PAGINATION regex compilation failed")
});

/// Evaluates page evidence and returns the winning [`PageIntent`] classification.
pub fn classify_intent(
    raw_html: &str,
    url: &str,
    page: &ParsedPage,
    cdp_signals: Option<&CdpIntentSignals>,
) -> PageIntent {
    let effective_html = if let Some(cdp) = cdp_signals {
        cdp.rendered_html.as_deref().unwrap_or(raw_html)
    } else {
        raw_html
    };

    let mut scores: HashMap<PageArchetype, f32> = HashMap::with_capacity(6);
    for &archetype in &ALL_ARCHETYPES {
        scores.insert(archetype, 0.0);
    }
    // Standard acts as the null hypothesis reference baseline prior
    scores.insert(PageArchetype::Standard, 1.5);

    let mut active_signals: Vec<CompactString> = Vec::with_capacity(8);

    // -------------------------------------------------------------
    // 0. CDP Runtime Globals & Visual CSSOM (Ground Truth)
    // -------------------------------------------------------------
    if let Some(cdp) = cdp_signals {
        if cdp.is_platform_pdp {
            *scores.entry(PageArchetype::Product).or_default() += 8.0;
            *scores.entry(PageArchetype::Article).or_default() -= 6.0;
            *scores.entry(PageArchetype::Category).or_default() -= 5.0;
            *scores.entry(PageArchetype::Standard).or_default() -= 4.0;
            active_signals.push(CompactString::new("cdp_runtime_platform_pdp"));
        }

        if cdp.is_platform_plp {
            *scores.entry(PageArchetype::Category).or_default() += 8.0;
            *scores.entry(PageArchetype::Product).or_default() -= 6.0;
            *scores.entry(PageArchetype::Article).or_default() -= 6.0;
            *scores.entry(PageArchetype::Standard).or_default() -= 4.0;
            active_signals.push(CompactString::new("cdp_runtime_platform_plp"));
        }

        if cdp.hero_price_above_fold {
            *scores.entry(PageArchetype::Product).or_default() += 4.5;
            *scores.entry(PageArchetype::Article).or_default() -= 3.0;
            active_signals.push(CompactString::new(format!(
                "cdp_visual_hero_price_above_fold({:.0}px)",
                cdp.hero_price_font_size
            )));
        }

        if cdp.uniform_card_count >= 4 {
            *scores.entry(PageArchetype::Category).or_default() += 5.0;
            *scores.entry(PageArchetype::Product).or_default() -= 4.0;
            *scores.entry(PageArchetype::Article).or_default() -= 4.0;
            active_signals.push(CompactString::new(format!(
                "cdp_visual_grid_uniformity({}_cards)",
                cdp.uniform_card_count
            )));
        }
    }

    // -------------------------------------------------------------
    // 1. Structural Path Priors
    // -------------------------------------------------------------
    let parsed_url = url::Url::parse(url).ok();
    let path = parsed_url.as_ref().map(|u| u.path()).unwrap_or("");
    let lower_path = path.to_ascii_lowercase();

    let is_root = path.is_empty() || path == "/" || path == "/index.html" || path == "/index.php";
    if is_root {
        *scores.entry(PageArchetype::Homepage).or_default() += 14.0;
        *scores.entry(PageArchetype::Standard).or_default() -= 8.0;
        *scores.entry(PageArchetype::Product).or_default() -= 8.0;
        *scores.entry(PageArchetype::Article).or_default() -= 10.0;
        *scores.entry(PageArchetype::Category).or_default() -= 6.0;
        *scores.entry(PageArchetype::Contact).or_default() -= 8.0;
        active_signals.push(CompactString::new("root_path"));
    } else {
        *scores.entry(PageArchetype::Homepage).or_default() -= 8.0;
    }

    let is_docs_or_legal = lower_path.starts_with("/docs")
        || lower_path.starts_with("/documentation")
        || lower_path.contains("/privacy")
        || lower_path.contains("/terms")
        || lower_path.contains("/legal")
        || lower_path.contains("/changelog")
        || lower_path.contains("/license")
        || lower_path.contains("/faq");

    if is_docs_or_legal {
        *scores.entry(PageArchetype::Standard).or_default() += 8.0;
        *scores.entry(PageArchetype::Article).or_default() -= 20.0;
        *scores.entry(PageArchetype::Product).or_default() -= 15.0;
        *scores.entry(PageArchetype::Category).or_default() -= 10.0;
        *scores.entry(PageArchetype::Contact).or_default() -= 8.0;
        active_signals.push(CompactString::new("docs_or_legal_path"));
    }

    let url_product = lower_path.contains("/product/")
        || lower_path.contains("/products/")
        || lower_path.contains("/item/")
        || lower_path.contains("/items/")
        || lower_path.contains("/p/")
        || lower_path.contains("/dp/")
        || lower_path.contains("/goods/");

    if url_product {
        *scores.entry(PageArchetype::Product).or_default() += 3.5;
        *scores.entry(PageArchetype::Category).or_default() -= 1.0;
        active_signals.push(CompactString::new("url_product_token"));
    }

    let url_category = lower_path.contains("/category/")
        || lower_path.contains("/categories/")
        || lower_path.contains("/collection/")
        || lower_path.contains("/collections/")
        || lower_path.contains("/shop/")
        || lower_path.contains("/catalog/")
        || lower_path.contains("/store/")
        || lower_path.contains("/browse/");

    if url_category && !url_product {
        *scores.entry(PageArchetype::Category).or_default() += 6.0;
        *scores.entry(PageArchetype::Product).or_default() -= 6.0;
        *scores.entry(PageArchetype::Standard).or_default() -= 2.0;
        active_signals.push(CompactString::new("url_category_token"));
    }

    let url_article = lower_path.contains("/blog/")
        || lower_path.contains("/news/")
        || lower_path.contains("/post/")
        || lower_path.contains("/posts/")
        || lower_path.contains("/article/")
        || lower_path.contains("/articles/")
        || lower_path.contains("/press/");

    if url_article {
        *scores.entry(PageArchetype::Article).or_default() += 4.0;
        active_signals.push(CompactString::new("url_article_token"));
    }

    let url_contact = lower_path.contains("/contact")
        || lower_path.contains("/contact-us")
        || lower_path.contains("/contactus")
        || lower_path.contains("/reach-us")
        || lower_path.contains("/get-in-touch");

    if url_contact {
        *scores.entry(PageArchetype::Contact).or_default() += 6.0;
        *scores.entry(PageArchetype::Article).or_default() -= 6.0;
        *scores.entry(PageArchetype::Product).or_default() -= 6.0;
        *scores.entry(PageArchetype::Category).or_default() -= 8.0;
        *scores.entry(PageArchetype::Standard).or_default() -= 3.0;
        active_signals.push(CompactString::new("url_contact_token"));
    }

    // -------------------------------------------------------------
    // 2. Spatial Transactional Micro-Cluster & Product Structure Evidence
    // -------------------------------------------------------------
    // Strip scripts and styles so minified code doesn't produce false price or text signals
    let clean_html = RE_SCRIPT_STYLE.replace_all(effective_html, " ");
    let clean_len = clean_html.len();

    let price_matches: Vec<usize> = RE_PRICE.find_iter(&clean_html).map(|m| m.start()).collect();
    let price_count = price_matches.len();

    let qty_matches: Vec<usize> = RE_QTY_INPUT
        .find_iter(&clean_html)
        .map(|m| m.start())
        .collect();
    let cta_matches: Vec<usize> = RE_CTA_BUTTON
        .find_iter(&clean_html)
        .map(|m| m.start())
        .collect();

    let mut has_micro_cluster = false;
    for &p_pos in &price_matches {
        for &c_pos in &cta_matches {
            let dist = p_pos.abs_diff(c_pos);
            if dist <= 1800 {
                if !qty_matches.is_empty() {
                    for &q_pos in &qty_matches {
                        let q_dist = p_pos.abs_diff(q_pos);
                        if q_dist <= 1800 {
                            has_micro_cluster = true;
                            break;
                        }
                    }
                } else {
                    has_micro_cluster = true;
                }
            }
            if has_micro_cluster {
                break;
            }
        }
        if has_micro_cluster {
            break;
        }
    }

    if has_micro_cluster {
        *scores.entry(PageArchetype::Product).or_default() += 6.0;
        *scores.entry(PageArchetype::Article).or_default() -= 5.0;
        *scores.entry(PageArchetype::Category).or_default() -= 3.0;
        *scores.entry(PageArchetype::Standard).or_default() -= 4.0;
        active_signals.push(CompactString::new("transactional_micro_cluster"));
    }

    if !qty_matches.is_empty() {
        *scores.entry(PageArchetype::Product).or_default() += 3.5;
        *scores.entry(PageArchetype::Article).or_default() -= 3.0;
        active_signals.push(CompactString::new("quantity_input"));
    }

    if !cta_matches.is_empty() && price_count > 0 {
        *scores.entry(PageArchetype::Product).or_default() += 3.5;
        *scores.entry(PageArchetype::Article).or_default() -= 2.0;
        active_signals.push(CompactString::new("action_button_with_price"));
    }

    // Hero Price Spatial Distribution:
    // Distinguishes a PDP with related items/recommendations in the footer
    // from a uniform multi-item category catalog.
    let mut is_hero_with_recommendations = false;
    if price_count >= 2 && clean_len > 0 {
        let first_price_pos = price_matches[0];
        let first_ratio = first_price_pos as f32 / clean_len as f32;
        let gap = price_matches[1].saturating_sub(first_price_pos);
        if (first_ratio <= 0.45 || first_price_pos <= 30_000)
            && (gap >= 3000 || gap as f32 / clean_len as f32 >= 0.20)
        {
            is_hero_with_recommendations = true;
        }
    }

    if is_hero_with_recommendations {
        *scores.entry(PageArchetype::Product).or_default() += 4.0;
        *scores.entry(PageArchetype::Category).or_default() -= 2.0;
        active_signals.push(CompactString::new("hero_price_with_recommendations"));
    } else if price_count == 1 {
        *scores.entry(PageArchetype::Product).or_default() += 2.5;
        *scores.entry(PageArchetype::Category).or_default() -= 1.0;
        active_signals.push(CompactString::new("single_hero_price"));
    } else if price_count > 0 {
        *scores.entry(PageArchetype::Product).or_default() += 2.0;
        *scores.entry(PageArchetype::Category).or_default() += 1.0;
        active_signals.push(CompactString::new("price_pattern"));
    }

    // Dense Technical Specification Matrix:
    // High-ticket B2B, industrial, machinery, automotive, and tech hardware pages
    // feature extensive key-value specification matrices (in <dl> or 2-column <table> rows).
    let dt_count = RE_DT_TAG.find_iter(&clean_html).count();
    let mut two_col_table_rows = 0;
    for cap in RE_TABLE_ROW.captures_iter(&clean_html) {
        if let Some(row_inner) = cap.get(1) {
            let cell_count = RE_TD_TH_TAG.find_iter(row_inner.as_str()).count();
            if cell_count == 2 {
                two_col_table_rows += 1;
            }
        }
    }
    let has_spec_matrix = dt_count >= 5 || two_col_table_rows >= 5;
    if has_spec_matrix {
        *scores.entry(PageArchetype::Product).or_default() += 4.5;
        *scores.entry(PageArchetype::Category).or_default() -= 3.5;
        *scores.entry(PageArchetype::Article).or_default() -= 3.0;
        active_signals.push(CompactString::new(format!(
            "dense_spec_matrix(dts={},two_col_rows={})",
            dt_count, two_col_table_rows
        )));
    }

    // Protocol-Level Action Anchors:
    // Direct transactional inquiry channels (WhatsApp, tel:, #quote, #contact)
    // used widely in B2B and high-ticket commerce where direct self-serve cart buttons are absent.
    let has_whatsapp = page.links.iter().any(|l| {
        let u = l.target_url.to_ascii_lowercase();
        u.contains("wa.me/") || u.contains("api.whatsapp.com/") || u.contains("whatsapp://")
    });
    let has_tel = page.links.iter().any(|l| l.target_url.starts_with("tel:"));
    let has_quote_or_inquiry = page.links.iter().any(|l| {
        let u = l.target_url.to_ascii_lowercase();
        u.contains("#contact")
            || u.contains("#quote")
            || u.contains("#inquiry")
            || u.contains("#inquire")
    });

    let has_b2b_action = has_whatsapp
        || ((has_tel || has_quote_or_inquiry)
            && (price_count > 0 || has_spec_matrix || url_product));
    if has_b2b_action {
        *scores.entry(PageArchetype::Product).or_default() += 4.0;
        *scores.entry(PageArchetype::Article).or_default() -= 2.0;
        active_signals.push(CompactString::new(if has_whatsapp {
            "protocol_whatsapp_inquiry"
        } else {
            "protocol_tel_quote_inquiry"
        }));
    }

    let has_availability = RE_AVAILABILITY.is_match(&clean_html);
    if has_availability {
        *scores.entry(PageArchetype::Product).or_default() += 3.5;
        *scores.entry(PageArchetype::Article).or_default() -= 2.0;
        active_signals.push(CompactString::new("availability_status"));
    }

    let og_type = page
        .get_open_graph("og:type")
        .map(|s| s.to_ascii_lowercase());
    if let Some(ref og) = og_type {
        if og.contains("product") {
            *scores.entry(PageArchetype::Product).or_default() += 5.5;
            *scores.entry(PageArchetype::Article).or_default() -= 4.0;
            *scores.entry(PageArchetype::Standard).or_default() -= 3.0;
            active_signals.push(CompactString::new("og_type_product"));
        } else if og.contains("article") {
            *scores.entry(PageArchetype::Article).or_default() += 5.5;
            *scores.entry(PageArchetype::Product).or_default() -= 4.0;
            *scores.entry(PageArchetype::Standard).or_default() -= 3.0;
            active_signals.push(CompactString::new("og_type_article"));
        }
    }

    // -------------------------------------------------------------
    // 3. Editorial Prose & Longform Analysis (Article Evidence)
    // -------------------------------------------------------------
    let has_byline = RE_BYLINE_AUTHOR.is_match(effective_html);
    let has_date = RE_DATE_PUBLISHED.is_match(effective_html)
        || effective_html.contains("<time")
        || effective_html.contains("datetime=");

    if has_byline {
        *scores.entry(PageArchetype::Article).or_default() += 3.5;
        active_signals.push(CompactString::new("byline_author"));
    }

    if has_date {
        *scores.entry(PageArchetype::Article).or_default() += 3.5;
        active_signals.push(CompactString::new("publication_date"));
    }

    // Count contiguous narrative prose runs (>= 120 chars)
    let long_chunks_count = count_long_text_chunks(effective_html, 120);

    let is_narrative = (long_chunks_count >= 3 && page.word_count > 300)
        || (long_chunks_count >= 1 && page.word_count > 250);

    if is_narrative {
        let has_editorial_marker = has_byline
            || has_date
            || effective_html.contains("<article")
            || effective_html.contains("role=\"article\"")
            || url_article
            || og_type
                .as_deref()
                .map(|s| s.contains("article"))
                .unwrap_or(false);

        if has_editorial_marker {
            *scores.entry(PageArchetype::Article).or_default() += 5.5;
            *scores.entry(PageArchetype::Product).or_default() -= 3.0;
            *scores.entry(PageArchetype::Category).or_default() -= 3.5;
            *scores.entry(PageArchetype::Standard).or_default() -= 2.0;
            active_signals.push(CompactString::new("continuous_narrative_runs"));
        } else {
            *scores.entry(PageArchetype::Standard).or_default() += 2.5;
            *scores.entry(PageArchetype::Article).or_default() += 1.0;
        }
    }

    if page.word_count > 1000 && long_chunks_count >= 4 && price_count <= 1 {
        *scores.entry(PageArchetype::Article).or_default() += 6.5;
        *scores.entry(PageArchetype::Category).or_default() -= 6.0;
        *scores.entry(PageArchetype::Product).or_default() -= 5.0;
        *scores.entry(PageArchetype::Standard).or_default() -= 3.0;
        active_signals.push(CompactString::new("encyclopedic_longform_text"));
    }

    // URL Leaf Slug <-> Primary H1 Semantic Token Overlap:
    // Leaf entity pages (specific PDPs or articles) have high lexical token overlap
    // between their path leaf slug and the main H1 headline.
    let (slug_token_count, slug_h1_overlap) =
        compute_slug_h1_overlap(url, page.h1_primary.as_deref());
    if slug_token_count >= 3 && slug_h1_overlap >= 0.50 {
        if url_product || price_count > 0 || has_spec_matrix {
            *scores.entry(PageArchetype::Product).or_default() += 4.0;
            *scores.entry(PageArchetype::Category).or_default() -= 4.0;
            active_signals.push(CompactString::new(format!(
                "slug_h1_high_overlap({:.0}%)",
                slug_h1_overlap * 100.0
            )));
        } else if url_article || has_byline || has_date {
            *scores.entry(PageArchetype::Article).or_default() += 4.0;
            *scores.entry(PageArchetype::Category).or_default() -= 4.0;
            active_signals.push(CompactString::new(format!(
                "slug_h1_high_overlap({:.0}%)",
                slug_h1_overlap * 100.0
            )));
        }
    }

    // -------------------------------------------------------------
    // 4. Catalog Grid & Link Density (Category Evidence)
    // -------------------------------------------------------------
    let link_count = page.links.len();
    let link_density = if page.word_count > 0 {
        link_count as f32 / page.word_count as f32
    } else {
        0.0
    };

    if price_count >= 4 && link_count >= 8 && !is_hero_with_recommendations {
        *scores.entry(PageArchetype::Article).or_default() -= 6.0;
        if !url_contact {
            *scores.entry(PageArchetype::Category).or_default() += 6.0;
        }
        *scores.entry(PageArchetype::Product).or_default() -= 6.0;
        *scores.entry(PageArchetype::Standard).or_default() -= 3.0;
        active_signals.push(CompactString::new("multi_price_catalog_grid"));
    }

    if link_density > 0.28 && link_count >= 6 {
        if !url_contact {
            *scores.entry(PageArchetype::Category).or_default() += 3.5;
        }
        *scores.entry(PageArchetype::Article).or_default() -= 3.0;
        *scores.entry(PageArchetype::Standard).or_default() -= 2.0;
        active_signals.push(CompactString::new("listing_link_density"));
    }

    let has_pagination = RE_PAGINATION.is_match(effective_html);
    if has_pagination && link_count >= 6 {
        if !url_contact {
            *scores.entry(PageArchetype::Category).or_default() += 4.0;
        }
        *scores.entry(PageArchetype::Product).or_default() -= 3.0;
        *scores.entry(PageArchetype::Standard).or_default() -= 2.0;
        active_signals.push(CompactString::new("pagination_controls"));
    }

    // -------------------------------------------------------------
    // 5. Contact Evidence
    // -------------------------------------------------------------
    let has_tel_or_mailto = page
        .links
        .iter()
        .any(|l| l.target_url.starts_with("tel:") || l.target_url.starts_with("mailto:"));
    if has_tel_or_mailto {
        *scores.entry(PageArchetype::Contact).or_default() += 3.5;
        active_signals.push(CompactString::new("tel_or_mailto"));
    }

    let has_contact_form = RE_CONTACT_FORM.is_match(effective_html);
    if has_contact_form {
        *scores.entry(PageArchetype::Contact).or_default() += 5.5;
        *scores.entry(PageArchetype::Product).or_default() -= 3.0;
        *scores.entry(PageArchetype::Article).or_default() -= 4.0;
        *scores.entry(PageArchetype::Standard).or_default() -= 3.0;
        active_signals.push(CompactString::new("contact_form"));
    }

    // -------------------------------------------------------------
    // 6. Softmax Normalization
    // -------------------------------------------------------------
    let max_score = scores.values().cloned().fold(f32::NEG_INFINITY, f32::max);

    let mut exp_sum = 0.0f32;
    let mut exp_scores: HashMap<PageArchetype, f32> = HashMap::with_capacity(6);
    for (&archetype, &s) in &scores {
        let e = (s - max_score).exp();
        exp_scores.insert(archetype, e);
        exp_sum += e;
    }

    let mut best_archetype = PageArchetype::Standard;
    let mut best_prob = 0.0f32;

    if exp_sum > 0.0 {
        for (&archetype, &e) in &exp_scores {
            let prob = e / exp_sum;
            if prob > best_prob {
                best_prob = prob;
                best_archetype = archetype;
            }
        }
    }

    PageIntent {
        archetype: best_archetype,
        confidence: best_prob,
        signals: active_signals,
    }
}

/// Helper function to count contiguous textual chunks between HTML tags of length >= `min_len`.
fn count_long_text_chunks(html: &str, min_len: usize) -> usize {
    let mut count = 0;
    let mut in_tag = false;
    let mut current_chunk_len = 0;

    for c in html.chars() {
        if c == '<' {
            in_tag = true;
            if current_chunk_len >= min_len {
                count += 1;
            }
            current_chunk_len = 0;
        } else if c == '>' {
            in_tag = false;
        } else if !in_tag && !c.is_whitespace() {
            current_chunk_len += 1;
        }
    }

    if current_chunk_len >= min_len {
        count += 1;
    }

    count
}

/// Computes the number of significant tokens in the URL's leaf slug, and the ratio of those
/// tokens that also appear in the primary H1 headline.
fn compute_slug_h1_overlap(url: &str, h1: Option<&str>) -> (usize, f32) {
    let parsed_url = match url::Url::parse(url) {
        Ok(u) => u,
        Err(_) => return (0, 0.0),
    };

    let path = parsed_url.path();
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    if segments.is_empty() {
        return (0, 0.0);
    }

    let mut leaf = *segments.last().unwrap_or(&"");
    if leaf.eq_ignore_ascii_case("index.html")
        || leaf.eq_ignore_ascii_case("index.php")
        || leaf.eq_ignore_ascii_case("index.htm")
    {
        if segments.len() >= 2 {
            leaf = segments[segments.len() - 2];
        } else {
            return (0, 0.0);
        }
    }

    let slug = if let Some(idx) = leaf.rfind('.') {
        &leaf[..idx]
    } else {
        leaf
    };

    let slug_tokens = tokenize_semantic_string(slug);
    if slug_tokens.len() < 3 {
        return (slug_tokens.len(), 0.0);
    }

    if let Some(h1_text) = h1 {
        let h1_tokens = tokenize_semantic_string(h1_text);
        if h1_tokens.is_empty() {
            return (slug_tokens.len(), 0.0);
        }

        let matches = slug_tokens
            .iter()
            .filter(|t| h1_tokens.contains(*t))
            .count();
        let ratio = matches as f32 / slug_tokens.len() as f32;
        return (slug_tokens.len(), ratio);
    }

    (slug_tokens.len(), 0.0)
}

/// Tokenizes a string into a set of lowercased alphanumeric tokens, splitting on non-alphanumerics,
/// separating merged digit-letter sequences (e.g. "1600kg" -> "1600", "kg"), and removing common stopwords.
fn tokenize_semantic_string(text: &str) -> std::collections::HashSet<CompactString> {
    let mut tokens = std::collections::HashSet::new();
    let stopwords = [
        "and", "or", "the", "a", "an", "of", "in", "for", "with", "to", "at", "by", "from", "on",
        "und", "der", "die", "das", "et", "le", "la", "les", "y", "el",
    ];

    for chunk in text.split(|c: char| !c.is_alphanumeric()) {
        let lower = chunk.to_ascii_lowercase();
        if lower.is_empty() {
            continue;
        }

        let mut sub_tokens: Vec<String> = Vec::new();
        let mut current = String::new();
        let mut prev_is_digit: Option<bool> = None;

        for ch in lower.chars() {
            let is_digit = ch.is_ascii_digit();
            if let Some(prev) = prev_is_digit {
                if prev != is_digit && !current.is_empty() {
                    sub_tokens.push(current.clone());
                    current.clear();
                }
            }
            current.push(ch);
            prev_is_digit = Some(is_digit);
        }
        if !current.is_empty() {
            sub_tokens.push(current);
        }

        if lower.len() >= 2 && !stopwords.contains(&lower.as_str()) {
            tokens.insert(CompactString::new(&lower));
        }

        if sub_tokens.len() > 1 {
            for sub in sub_tokens {
                if sub.len() >= 2 && !stopwords.contains(&sub.as_str()) {
                    tokens.insert(CompactString::new(sub));
                }
            }
        }
    }

    tokens
}
