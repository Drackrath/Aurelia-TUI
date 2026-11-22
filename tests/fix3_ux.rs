//! Footer hints, label gaps, long inputs, account cache.

use aurelia_tui::browse::Browser;
use aurelia_tui::interface::aurelia::{AccountJson, LibraryGameJson};
use aurelia_tui::interface::game::Game;
use aurelia_tui::settings::{self, Setting};
use aurelia_tui::ui;
use tui::Terminal;
use tui::backend::TestBackend;
use tui::layout::Rect;
use tui::widgets::Widget;

const SIZES: [(u16, u16); 2] = [(120, 40), (80, 24)];

/// Serializes tests sharing the config.
static CONFIG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn isolate_config() -> std::sync::MutexGuard<'static, ()> {
    let guard = CONFIG_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("fix3-ux-config");
    // SAFETY: set before Config::cached().
    unsafe {
        std::env::set_var("AURELIA_TUI_DIR", &dir);
        std::env::set_var("AURELIA_TUI_CACHE_DIR", &dir);
        std::env::set_var("AURELIA_BIN", dir.join("missing-aurelia"));
    }
    guard
}

fn game() -> Game {
    Game::from_library(
        serde_json::from_value::<LibraryGameJson>(serde_json::json!({
            "app_id": 1, "name": "Alpha", "is_installed": true
        }))
        .unwrap(),
    )
}

fn rows_of(w: u16, h: u16, area: Rect, widget: impl Widget) -> Vec<String> {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| f.render_widget(widget, area)).unwrap();
    let buf = t.backend().buffer();
    (0..h)
        .map(|y| (0..w).map(|x| buf.get(x, y).symbol.clone()).collect())
        .collect()
}

#[test]
fn footer_keeps_help_and_quit() {
    let _lock = isolate_config();
    let b = Browser::new(vec![game()]);
    for (w, _) in SIZES {
        let area = Rect::new(0, 0, w, 2);
        let rows = rows_of(
            w,
            2,
            area,
            ui::status::status_bar_fit(&b, Some("tester"), area, Some("0.3.1")),
        );
        assert!(rows[1].contains("[?] help"), "{w}: {}", rows[1]);
        assert!(rows[1].contains("[q] quit"), "{w}: {}", rows[1]);
    }
    let hints = [("[a]", " aaaa  "), ("[?]", " help  "), ("[q]", " quit")];
    assert_eq!(ui::status::fit_hints(&hints, 20).len(), 2);
}

#[test]
fn settings_labels_never_touch_keys() {
    let _lock = isolate_config();
    let mut b = Browser::new(vec![]);
    b.show_config = true;
    for (w, h) in SIZES {
        let area = ui::centered_rect(80, 80, Rect::new(0, 0, w, h));
        let rows = rows_of(
            w,
            h,
            area,
            ui::config::config_sized(&b, area.width, area.height),
        );
        let text = rows.join("\n");
        assert!(text.contains("Play (TUI) [t]"), "{text}");
    }
    for s in settings::TUI_ROWS
        .into_iter()
        .chain(settings::LAUNCHER_ROWS)
        .chain([Setting::DownloadLimit])
    {
        assert!(s.label().chars().count() <= 28, "{}", s.label());
    }
}

#[test]
fn long_inputs_keep_caret_visible() {
    let _lock = isolate_config();
    let long = format!("/home/user/{}/target/release/aurelia", "deep/".repeat(30));
    let mut b = Browser::new(vec![]);
    b.show_config = true;
    b.config_index = 3;
    b.config_binary_input = Some(long.clone());
    for (w, h) in SIZES {
        let area = ui::centered_rect(80, 80, Rect::new(0, 0, w, h));
        let rows = rows_of(
            w,
            h,
            area,
            ui::config::config_sized(&b, area.width, area.height),
        );
        let row = rows.iter().find(|r| r.contains("Aurelia binary")).unwrap();
        assert!(
            row.contains("…") && row.contains("release/aurelia_"),
            "{row}"
        );
        let area = ui::centered_rect(60, 30, Rect::new(0, 0, w, h));
        let prompt = ui::prompt_overlay("Path:", &long, "", "Move");
        let text = rows_of(w, h, area, prompt).join("\n");
        assert!(text.contains("release/aurelia_"), "{text}");
    }
    assert_eq!(ui::input_tail("abc", 10), "abc_");
    assert!(ui::input_tail(&long, 10).starts_with('…'));
}

#[test]
fn account_is_cached_for_the_session() {
    let _lock = isolate_config();
    let mut b = Browser::new(vec![]);
    b.open_account();
    assert!(b.account_load.loading);
    let info: AccountJson = serde_json::from_str(r#"{"account_name":"tester"}"#).unwrap();
    b.finish_account_for_test(Ok(info));
    b.poll_account();
    assert!(!b.account_load.loading);
    b.close_account();
    b.open_account();
    assert!(!b.account_load.loading, "reopen is instant");
    assert_eq!(b.account_info.as_ref().unwrap().account_name, "tester");
    for (w, h) in SIZES {
        let area = ui::centered_rect(64, 55, Rect::new(0, 0, w, h));
        let text = rows_of(w, h, area, ui::account::account(&b)).join("\n");
        assert!(text.contains("[Esc] close"), "{w}x{h}\n{text}");
        assert!(text.contains("[r] reload"));
    }
    b.expire_account_for_test();
    b.close_account();
    b.open_account();
    assert!(b.account_load.loading, "stale cache refetches");
    b.forget_account();
    assert!(b.account_info.is_none());
}
