//! Expanded detail scroll, real path.
//! Own binary: scroll limits are process-wide.

use aurelia_tui::browse::Browser;
use aurelia_tui::interface::aurelia::LibraryGameJson;
use aurelia_tui::interface::game::Game;
use aurelia_tui::interface::library_info::{self as info, ExtendedInfoJson, RequirementsJson};
use aurelia_tui::ui;
use tui::{Terminal, backend::TestBackend};

fn isolate_config() {
    let dir = std::env::temp_dir().join(format!("aurelia-library-scroll-{}", std::process::id()));
    // SAFETY: set before any config read.
    unsafe {
        std::env::set_var("AURELIA_TUI_DIR", &dir);
        std::env::set_var("AURELIA_TUI_CACHE_DIR", &dir);
        std::env::set_var("AURELIA_BIN", dir.join("missing-backend"));
    }
}

fn game(id: u32) -> Game {
    let entry: LibraryGameJson = serde_json::from_value(serde_json::json!({
        "app_id": id,
        "name": "Baldur's Gate 3",
        "is_installed": true,
        "install_path": "/games/bg3",
        "playtime_forever_minutes": 8198,
        "size_on_disk": 155_190_000_000u64,
        "store_url": format!("https://store.steampowered.com/app/{id}"),
    }))
    .unwrap();
    Game::from_library(entry)
}

fn extended() -> ExtendedInfoJson {
    ExtendedInfoJson {
        genres: vec!["Adventure".into(), "RPG".into()],
        categories: (0..40).map(|i| format!("Feature {i}")).collect(),
        metacritic: Some(96),
        website: Some("https://baldursgate3.game".into()),
        requirements: RequirementsJson {
            minimum: (0..9).map(|i| format!("Min line {i}")).collect(),
            recommended: vec!["Rec last line".into()],
        },
        ..Default::default()
    }
}

/// One frame, ordered like main.rs.
fn frame(browser: &Browser, w: u16, h: u16) -> String {
    let g = browser.selected().unwrap();
    let pane_w = w * 6 / 10;
    let pane_h = h - 7;
    let value_w = (pane_w - 2) * 82 / 100 - 1;
    let mut terminal = Terminal::new(TestBackend::new(pane_w, pane_h)).unwrap();
    terminal
        .draw(|f| {
            let _ = ui::detail::content_height(Some(&g), browser.expand_description, value_w);
            let scroll = browser.detail_scroll_for(Some(g.id));
            f.render_widget(
                ui::detail::detail_scrolled(
                    Some(&g),
                    browser.expand_description,
                    value_w,
                    pane_h - 2,
                    scroll,
                ),
                f.size(),
            )
        })
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    let mut text = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            text.push_str(&buffer.get(x, y).symbol);
        }
        text.push('\n');
    }
    text
}

#[test]
fn pgdn_reaches_last_requirement_at_both_sizes() {
    isolate_config();
    for (w, h) in [(120u16, 40u16), (80, 24)] {
        let id = 1_086_940;
        info::store_extended(id, None, Some(extended()));
        let mut browser = Browser::new(vec![game(id as u32)]);
        browser.toggle_description();
        let first = frame(&browser, w, h);
        assert!(first.contains("more · PgDn"), "{w}x{h}: {first}");
        assert!(!first.contains("Rec last line"), "{w}x{h}: {first}");
        // Each line shows on some page.
        let mut seen = first.clone();
        let mut last = first;
        for _ in 0..40 {
            browser.detail_scroll_by(1);
            last = frame(&browser, w, h);
            seen.push_str(&last);
        }
        assert!(last.contains("Rec last line"), "{w}x{h} end: {last}");
        assert!(!last.contains("more · PgDn"), "{w}x{h} end: {last}");
        for i in 0..9 {
            assert!(seen.contains(&format!("Min line {i}")), "{w}x{h} line {i}");
        }
        assert!(seen.contains("Feature 39"), "{w}x{h}");
        // Labels fit the label column.
        for label in ["Develope ", "Publishe ", "Proton T ", "Installa "] {
            assert!(!seen.contains(label), "{w}x{h} clipped {label}: {seen}");
        }
        // PgUp returns to the top.
        for _ in 0..40 {
            browser.detail_scroll_by(-1);
        }
        let top = frame(&browser, w, h);
        assert!(top.contains("Playtime"), "{w}x{h}: {top}");
    }
}
