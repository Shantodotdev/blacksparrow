//! # Terminal Single-Line Progress & Display Width Tests
//!
//! Validates that live crawler progress indicators strictly fit within terminal column
//! limits (including 80-column standards and narrow split-panes) and never wrap or scroll.

use blacksparrow::crawler::engine::ProgressUpdate;
use blacksparrow::report::terminal::{
    format_crawl_progress, truncate_to_visible_width, visible_width,
};

#[test]
fn test_visible_width_calculation() {
    let plain = "Hello, world!";
    assert_eq!(visible_width(plain), 13);

    let styled = "\x1b[38;5;198m\x1b[1mSPARROW\x1b[0m";
    assert_eq!(visible_width(styled), 7);

    // Emojis: 🚨 (width 2) + space (1) + 123 (3) = 6
    let emoji_styled = "\x1b[38;5;196m🚨 123\x1b[0m";
    assert_eq!(visible_width(emoji_styled), 6);
}

#[test]
fn test_truncate_to_visible_width() {
    let text = " \x1b[38;5;198m⠋\x1b[0m [00:05] [▰▰▱▱▱▱▱▱] 854/4455 (19%) │ 150/s";
    let w = visible_width(text);
    assert!(w > 30);

    let truncated_40 = truncate_to_visible_width(text, 40);
    assert!(visible_width(&truncated_40) <= 40);
    assert!(truncated_40.ends_with("\x1b[0m"));

    let untruncated = truncate_to_visible_width(text, 100);
    assert_eq!(visible_width(&untruncated), w);
}

#[test]
fn test_progress_line_strictly_fits_80_col_terminal() {
    // Standard 80-column terminal with large counts and multi-defect badges
    let update = ProgressUpdate {
        crawled_pages: 854,
        discovered_pages: 4455,
        max_pages: 50000,
        current_url: "http://localhost:3001/catalog/item-42".into(),
        status_code: 200,
        ttfb_ms: 12,
        aimd_delay_ms: 0,
        critical_count: 893,
        alert_count: 160,
        warning_count: 200,
    };

    let formatted = format_crawl_progress(&update, "00:05", "⠋", 150, 50000, 80);
    let width = visible_width(&formatted);

    // Must never contain a newline character
    assert!(!formatted.contains('\n'));
    assert!(!formatted.contains('\r'));

    // Must strictly fit inside 80 columns with safety margin (<= 78 columns)
    assert!(
        width <= 78,
        "Progress line width ({width}) exceeds safe 80-column boundary (78 cols): {formatted}"
    );
}

#[test]
fn test_progress_line_extreme_numbers_fits_80_col_terminal() {
    // 5-digit pages, 4-digit issues
    let update = ProgressUpdate {
        crawled_pages: 50000,
        discovered_pages: 50000,
        max_pages: 50000,
        current_url: "http://localhost:3001/".into(),
        status_code: 200,
        ttfb_ms: 5,
        aimd_delay_ms: 0,
        critical_count: 9999,
        alert_count: 9999,
        warning_count: 9999,
    };

    let formatted = format_crawl_progress(&update, "59:59", "⠙", 1250, 50000, 80);
    let width = visible_width(&formatted);

    assert!(!formatted.contains('\n'));
    assert!(!formatted.contains('\r'));
    assert!(
        width <= 78,
        "Extreme numbers progress line width ({width}) exceeds safe 78 cols: {formatted}"
    );
}

#[test]
fn test_progress_line_responsive_narrow_terminal() {
    let update = ProgressUpdate {
        crawled_pages: 350,
        discovered_pages: 1200,
        max_pages: 2000,
        current_url: "http://localhost:3001/".into(),
        status_code: 200,
        ttfb_ms: 20,
        aimd_delay_ms: 0,
        critical_count: 12,
        alert_count: 4,
        warning_count: 30,
    };

    // Split pane width 65
    let formatted_65 = format_crawl_progress(&update, "00:10", "⠹", 35, 2000, 65);
    let width_65 = visible_width(&formatted_65);
    assert!(!formatted_65.contains('\n'));
    assert!(
        width_65 <= 63,
        "Width {width_65} exceeds 63 cols for 65-column terminal"
    );

    // Very narrow split pane width 48
    let formatted_48 = format_crawl_progress(&update, "00:10", "⠹", 35, 2000, 48);
    let width_48 = visible_width(&formatted_48);
    assert!(!formatted_48.contains('\n'));
    assert!(
        width_48 <= 46,
        "Width {width_48} exceeds 46 cols for 48-column terminal"
    );
}

#[test]
fn test_progress_line_unlimited_crawl() {
    let update_undiscovered = ProgressUpdate {
        crawled_pages: 1420,
        discovered_pages: 0,
        max_pages: 0, // unlimited
        current_url: "http://localhost:3001/".into(),
        status_code: 200,
        ttfb_ms: 15,
        aimd_delay_ms: 0,
        critical_count: 5,
        alert_count: 0,
        warning_count: 18,
    };

    let formatted = format_crawl_progress(&update_undiscovered, "01:22", "⠴", 95, 0, 80);
    let width = visible_width(&formatted);
    assert!(!formatted.contains('\n'));
    assert!(width <= 78);
    assert!(formatted.contains("1420 pages"));

    let update_discovered = ProgressUpdate {
        crawled_pages: 1420,
        discovered_pages: 5000,
        max_pages: 0,
        current_url: "http://localhost:3001/".into(),
        status_code: 200,
        ttfb_ms: 15,
        aimd_delay_ms: 0,
        critical_count: 5,
        alert_count: 0,
        warning_count: 18,
    };

    let formatted_disc = format_crawl_progress(&update_discovered, "01:22", "⠴", 95, 0, 80);
    let width_disc = visible_width(&formatted_disc);
    assert!(!formatted_disc.contains('\n'));
    assert!(width_disc <= 78);
    assert!(formatted_disc.contains("1420/5000"));
}
