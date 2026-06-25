//! Exercises the Community Market search overlay: the lenient result/price
//! structs, the browse-state transitions, and the widget render. No live CLI is
//! involved — the search/price worker cells are driven through the public state
//! API so the test is hermetic.

use aurelia_tui::browse::Browser;
use aurelia_tui::interface::aurelia::{MarketPriceJson, MarketSearchResultJson};
use aurelia_tui::ui;
use serde_json::json;
use tui::backend::TestBackend;
use tui::layout::Rect;
use tui::Terminal;

/// A partial/empty JSON payload must still deserialize (every field defaulted).
#[test]
fn search_result_tolerates_partial_json() {
    // Empty object: all fields default.
    let empty: MarketSearchResultJson = serde_json::from_value(json!({})).unwrap();
    assert_eq!(empty.app_id, 0);
    assert!(empty.market_hash_name.is_empty());
    assert_eq!(empty.display_name(), "(unnamed item)");
    assert!(empty.price_text().is_empty());

    // Full row from the real CLI shape.
    let full: MarketSearchResultJson = serde_json::from_value(json!({
        "app_id": 440,
        "app_name": "Team Fortress 2",
        "market_hash_name": "Mann Co. Supply Crate Key",
        "name": "Mann Co. Supply Crate Key",
        "sell_listings": 37094,
        "sell_price": 234,
        "sell_price_text": "$2.34"
    }))
    .unwrap();
    assert_eq!(full.app_id, 440);
    assert_eq!(full.display_name(), "Mann Co. Supply Crate Key");
    assert_eq!(full.price_text(), "$2.34");

    // Missing pre-formatted text falls back to a minor-unit decimal.
    let no_text: MarketSearchResultJson = serde_json::from_value(json!({
        "name": "Thing",
        "sell_price": 199
    }))
    .unwrap();
    assert_eq!(no_text.price_text(), "1.99");
}

/// A price payload that carries no usable fields summarises to `None`; a full one
/// produces a one-line summary.
#[test]
fn price_summary_handles_empty_and_full() {
    let empty: MarketPriceJson = serde_json::from_value(json!({})).unwrap();
    assert!(empty.summary().is_none());

    let full: MarketPriceJson = serde_json::from_value(json!({
        "market_hash_name": "Mann Co. Supply Crate Key",
        "lowest_price": "$2.34",
        "median_price": "$2.34",
        "volume": "69,730"
    }))
    .unwrap();
    let summary = full.summary().expect("full payload summarises");
    assert!(summary.contains("low $2.34"));
    assert!(summary.contains("median $2.34"));
    assert!(summary.contains("vol 69,730"));
}

/// Opening the overlay clears state; the query mutators behave; navigation is
/// clamped on an empty result set.
#[test]
fn overlay_state_transitions() {
    let mut browser = Browser::new(Vec::new());
    assert!(!browser.show_market_search);

    browser.open_market_search();
    assert!(browser.show_market_search);
    assert!(browser.market_query.is_empty());

    browser.market_query_push('k');
    browser.market_query_push('e');
    browser.market_query_push('y');
    assert_eq!(browser.market_query, "key");
    browser.market_query_pop();
    assert_eq!(browser.market_query, "ke");

    // No results yet: navigation and price lookup are no-ops, not panics.
    browser.market_result_next();
    browser.market_result_previous();
    assert!(browser.selected_market_result().is_none());
    browser.lookup_market_price();

    browser.close_market_search();
    assert!(!browser.show_market_search);
    assert!(browser.market_query.is_empty());
}

/// The widget renders the query, status, and result rows without panicking, and
/// shows the resolved item names + prices.
#[test]
fn overlay_renders_query_and_results() {
    let mut browser = Browser::new(Vec::new());
    browser.open_market_search();
    browser.market_query_push('M');
    browser.market_query_push('a');
    browser.market_query_push('n');
    browser.market_query_push('n');
    browser.market_search_status = "2 results".to_string();
    // Inject resolved results directly (the worker cell normally does this).
    browser.market_results = vec![
        MarketSearchResultJson {
            app_id: 440,
            app_name: "Team Fortress 2".to_string(),
            market_hash_name: "Mann Co. Supply Crate Key".to_string(),
            name: "Mann Co. Supply Crate Key".to_string(),
            sell_listings: 37094,
            sell_price: 234,
            sell_price_text: "$2.34".to_string(),
        },
        MarketSearchResultJson {
            app_id: 440,
            app_name: "Team Fortress 2".to_string(),
            market_hash_name: "Mann Co. Store Package".to_string(),
            name: "Mann Co. Store Package".to_string(),
            sell_listings: 35922,
            sell_price: 9,
            sell_price_text: "$0.09".to_string(),
        },
    ];

    let backend = TestBackend::new(70, 20);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            f.render_widget(
                ui::market_search::market_search(&browser, 18),
                Rect::new(0, 0, 70, 20),
            );
        })
        .unwrap();

    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol.clone())
        .collect();

    assert!(text.contains("Search:"), "query input label renders");
    assert!(text.contains("Mann"), "typed query / result name renders");
    assert!(text.contains("$2.34"), "result price renders");
    assert!(text.contains("results"), "status line renders");
    assert!(text.contains("Market search"), "panel title renders");
}
