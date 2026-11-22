//! Fit and scroll checks at 120x40, 80x24.

use aurelia_tui::browse::{Action, Browser};
use aurelia_tui::config::Config;
use aurelia_tui::interface::aurelia::LibraryGameJson;
use aurelia_tui::interface::game::Game;
use aurelia_tui::settings::{self, Setting};
use aurelia_tui::ui;
use crossterm::event::KeyCode;
use tui::Terminal;
use tui::backend::TestBackend;
use tui::layout::Rect;
use tui::widgets::Paragraph;

const SIZES: [(u16, u16); 2] = [(120, 40), (80, 24)];

fn isolate_config() {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("fix-ux-config");
    // SAFETY: set before Config::cached().
    unsafe {
        std::env::set_var("AURELIA_TUI_DIR", &dir);
        std::env::set_var("AURELIA_TUI_CACHE_DIR", &dir);
        std::env::set_var("AURELIA_BIN", dir.join("missing-aurelia"));
    }
}

fn game(id: i32, name: &str) -> Game {
    Game::from_library(
        serde_json::from_value::<LibraryGameJson>(serde_json::json!({
            "app_id": id, "name": name, "is_installed": true
        }))
        .unwrap(),
    )
}

/// Render into the popup main.rs uses.
fn popup(
    size: (u16, u16),
    pct: (u16, u16),
    build: impl FnOnce(Rect) -> Paragraph<'static>,
) -> Vec<String> {
    let (w, h) = size;
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    let area = ui::centered_rect(pct.0, pct.1, Rect::new(0, 0, w, h));
    terminal
        .draw(|f| f.render_widget(build(area), area))
        .unwrap();
    let buf = terminal.backend().buffer();
    (area.y..area.y + area.height)
        .map(|y| {
            (area.x..area.x + area.width)
                .map(|x| buf.get(x, y).symbol.clone())
                .collect()
        })
        .collect()
}

#[test]
fn actions_scroll_to_end_without_wrap() {
    isolate_config();
    for size in SIZES {
        let mut b = Browser::new(vec![game(1, "Alpha")]);
        b.open_actions();
        b.actions_previous();
        assert_eq!(b.actions_index, 0, "Up from top stays put");
        let n = b.filtered_actions().len();
        for _ in 0..n + 5 {
            b.actions_next();
        }
        assert_eq!(b.actions_index, n - 1, "Down stops at end");
        assert_eq!(b.selected_action(), Some(Action::Help));
        let rows = popup(size, (70, 84), |a| ui::actions::actions_in(&b, a.height));
        let text = rows.join("\n");
        assert!(text.contains("▶ Key help"), "{size:?}\n{text}");
        assert!(
            text.contains("\nApp") || text.contains("│App"),
            "sticky header\n{text}"
        );
        assert!(rows[0].contains(&format!("({n}/{n})")), "{}", rows[0]);
        assert!(text.contains("type to filter"), "hint fits\n{text}");
        // Middle row keeps its header.
        b.actions_index = n / 2;
        let text = popup(size, (70, 84), |a| ui::actions::actions_in(&b, a.height)).join("\n");
        assert!(text.contains('▶'), "middle selection visible\n{text}");
    }
}

#[test]
fn reconnect_asks_first() {
    let rows = popup((80, 24), (50, 30), |_| ui::confirm::confirm_reconnect());
    let text = rows.join("\n");
    assert!(text.contains("Reconnect"), "{text}");
    assert!(text.contains("[y]"));
}

#[test]
fn help_fits_and_keeps_groups() {
    for (w, h) in SIZES {
        let area = ui::centered_rect(90, 90, Rect::new(0, 0, w, h));
        let inner = usize::from(area.width) - 2;
        let lines = ui::help::lines(inner);
        for line in &lines {
            let len: usize = line.0.iter().map(|s| s.content.chars().count()).sum();
            assert!(len <= inner, "{w}x{h}: line {len} > {inner}");
        }
        let text: Vec<String> = lines
            .iter()
            .map(|l| l.0.iter().map(|s| s.content.as_ref()).collect())
            .collect();
        let all = text.join("\n");
        assert!(all.contains("Depot files: b d  branch"), "key gap");
        assert!(all.contains("Add friend Enter  search"), "key gap");
        // Headers keep an entry below.
        let col = ui::help::COL_WIDTH;
        let columns = if inner >= col * 2 + 2 { 2 } else { 1 };
        for c in 0..columns {
            let cell = |row: &String| -> String {
                row.chars()
                    .skip(c * (col + 2))
                    .take(col)
                    .collect::<String>()
            };
            for (i, row) in text.iter().enumerate() {
                let here = cell(row);
                let header = !here.starts_with("  ") && !here.trim().is_empty();
                if header {
                    let next = text.get(i + 1).map(cell).unwrap_or_default();
                    assert!(next.starts_with("  ") && !next.trim().is_empty(), "{here}");
                }
            }
        }
        let max = ui::help::max_scroll(area.width, area.height);
        let rows = popup((w, h), (90, 90), |a| {
            ui::help::help_in(max, a.width, a.height)
        });
        let joined = rows.join("\n");
        assert!(joined.contains("re-check session"), "last group\n{joined}");
        assert!(rows[0].contains("Esc/q/? close"), "{}", rows[0]);
    }
}

#[test]
fn account_opens_at_once_and_reports_errors() {
    isolate_config();
    for size in SIZES {
        let mut b = Browser::new(vec![]);
        b.open_account();
        assert!(b.show_account && b.account_load.loading);
        let draw = |b: &Browser| {
            let mut t = Terminal::new(TestBackend::new(size.0, size.1)).unwrap();
            let area = ui::centered_rect(50, 55, Rect::new(0, 0, size.0, size.1));
            t.draw(|f| f.render_widget(ui::account::account(b), area))
                .unwrap();
            t.backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol.clone())
                .collect::<String>()
        };
        assert!(draw(&b).contains("[Esc] close"), "title fits");
        b.finish_account_for_test(Err("no session".into()));
        b.poll_account();
        assert!(!b.account_load.loading);
        assert!(b.account_load.error.is_some());
        assert!(b.notice.as_deref().unwrap().starts_with("Account:"));
        assert!(draw(&b).contains("Failed"));
        b.close_account();
        assert!(!b.show_account);
    }
}

fn launcher_browser() -> Browser {
    let mut b = Browser::new(vec![]);
    let path = format!(
        "{}/tests/fixtures/play_settings/config_show.json",
        env!("CARGO_MANIFEST_DIR")
    );
    b.config_info = Some(serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap());
    b.show_config = true;
    b
}

#[test]
fn settings_fit_both_sizes() {
    isolate_config();
    for size in SIZES {
        let mut b = launcher_browser();
        for _ in 0..40 {
            settings::next(&mut b);
        }
        assert_eq!(settings::selected(&b), Some(Setting::Proxy));
        let rows = popup(size, (80, 80), |a| ui::config::config_in(&b, a.height));
        let text = rows.join("\n");
        assert!(rows[0].contains("[Esc] close"), "{}", rows[0]);
        assert!(text.contains("▶ Proxy"), "{size:?}\n{text}");
        assert!(text.contains(Setting::Proxy.help()), "{text}");
        assert!(text.contains("[e]"), "shortcut shown");
        assert!(text.contains("Sync saves around Play (TUI)") || size.1 < 30);
    }
}

#[test]
fn store_language_picker_saves_on_enter_only() {
    isolate_config();
    let mut config = Config::default();
    config.save().unwrap();
    let mut b = launcher_browser();
    settings::apply(Setting::StoreLanguage, &mut b, &mut config);
    assert_eq!(b.config_language_pick, Some(0));
    for _ in 0..100 {
        settings::language_key(&mut b, &mut config, KeyCode::Down);
    }
    let last = settings::language_choices().len() - 1;
    assert_eq!(b.config_language_pick, Some(last));
    assert!(config.info_language.is_none(), "moving does not save");
    for size in SIZES {
        let rows = popup(size, (80, 80), |a| ui::config::config_in(&b, a.height));
        let text = rows.join("\n");
        assert!(text.contains('▶'), "picked row visible\n{text}");
        assert!(rows[0].contains("[Esc] cancel"), "{}", rows[0]);
    }
    settings::language_key(&mut b, &mut config, KeyCode::Esc);
    assert!(b.config_language_pick.is_none());
    assert!(config.info_language.is_none(), "Esc keeps value");

    settings::apply(Setting::StoreLanguage, &mut b, &mut config);
    settings::language_key(&mut b, &mut config, KeyCode::Down);
    settings::language_key(&mut b, &mut config, KeyCode::Enter);
    assert_eq!(config.info_language.as_deref(), Some("english"));
}

#[test]
fn binary_input_prefills_and_keeps_unchanged() {
    isolate_config();
    let mut config = Config {
        aurelia_bin: Some("/opt/aurelia".into()),
        ..Config::default()
    };
    config.save().unwrap();
    let mut b = launcher_browser();
    settings::apply(Setting::Binary, &mut b, &mut config);
    assert_eq!(b.config_binary_input.as_deref(), Some("/opt/aurelia"));
    settings::commit_binary(&mut b, &mut config);
    assert_eq!(config.aurelia_bin.as_deref(), Some("/opt/aurelia"));
    assert!(b.notice.as_deref().unwrap().contains("unchanged"));
}

#[test]
fn limit_input_hint_once() {
    isolate_config();
    let mut b = launcher_browser();
    b.download_limit.input = Some("9".into());
    let value = ui::install_queue::limit_value(&b);
    assert!(!value.contains("[Enter]"), "{value}");
}

#[test]
fn status_bar_drops_low_priority_parts() {
    isolate_config();
    let mut b = Browser::new(vec![game(1, "Alpha")]);
    b.notice = Some("x".repeat(30));
    for (w, _) in SIZES {
        let area = Rect::new(0, 0, w, 2);
        let mut t = Terminal::new(TestBackend::new(w, 2)).unwrap();
        t.draw(|f| {
            f.render_widget(
                ui::status::status_bar_fit(&b, Some("tester"), area, Some("0.3.1")),
                area,
            )
        })
        .unwrap();
        let top: String = (0..w)
            .map(|x| t.backend().buffer().get(x, 0).symbol.clone())
            .collect();
        assert!(top.contains("shown"), "{top}");
        assert!(!top.contains("sort: nam ") || top.contains("sort: name"));
    }
    b.notice = Some("y".repeat(200));
    let narrow = ui::status::status_bar_in(&b, None, 40);
    let mut t = Terminal::new(TestBackend::new(40, 2)).unwrap();
    t.draw(|f| f.render_widget(narrow, Rect::new(0, 0, 40, 2)))
        .unwrap();
    let top: String = (0..40)
        .map(|x| t.backend().buffer().get(x, 0).symbol.clone())
        .collect();
    assert!(top.trim_end().ends_with('…'), "{top}");
}
