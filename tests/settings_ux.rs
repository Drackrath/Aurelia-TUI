//! Selectable Settings rows, actions menu, overlays.

use aurelia_tui::browse::{Action, Browser};
use aurelia_tui::config::Config;
use aurelia_tui::interface::game::Game;
use aurelia_tui::settings::{self, Kind, Setting};
use aurelia_tui::ui;
use tui::Terminal;
use tui::backend::TestBackend;
use tui::layout::Rect;

fn isolate_config() {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("settings-ux-config");
    // SAFETY: set before Config::cached().
    unsafe {
        std::env::set_var("AURELIA_TUI_DIR", &dir);
        std::env::set_var("AURELIA_TUI_CACHE_DIR", &dir);
        std::env::set_var("AURELIA_BIN", dir.join("missing-aurelia"));
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

fn render(b: &Browser, w: u16, h: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal
        .draw(|f| f.render_widget(ui::config::config_in(b, h), Rect::new(0, 0, w, h)))
        .unwrap();
    let buf = terminal.backend().buffer();
    (0..h)
        .map(|y| {
            (0..w)
                .map(|x| buf.get(x, y).symbol.clone())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn rows_are_grouped_and_ordered() {
    isolate_config();
    let b = launcher_browser();
    let rows = settings::selectable(&b);
    assert_eq!(rows[..4], settings::TUI_ROWS);
    assert_eq!(
        rows[4..],
        [
            Setting::Presence,
            Setting::PersonaWait,
            Setting::CloudSync,
            Setting::DownloadLimit,
            Setting::Shaders,
            Setting::AntiCheat,
            Setting::Proxy,
        ]
    );
    let out = render(&b, 100, 40);
    for header in ["TUI", "Steam / Aurelia", "Info (read-only)"] {
        assert!(out.contains(header), "{header}\n{out}");
    }
    assert!(out.contains(ui::config::TITLE), "short title\n{out}");
    assert!(out.contains("Library"), "info rows render");
}

#[test]
fn without_launcher_only_local_rows() {
    isolate_config();
    let mut b = Browser::new(vec![]);
    b.show_config = true;
    let rows = settings::selectable(&b);
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[4], Setting::DownloadLimit);
    assert!(render(&b, 100, 30).contains("Unavailable"));
}

#[test]
fn selection_wraps_and_shows_help() {
    isolate_config();
    let mut b = launcher_browser();
    assert_eq!(settings::selected(&b), Some(Setting::Hltb));
    settings::previous(&mut b);
    assert_eq!(settings::selected(&b), Some(Setting::Proxy));
    settings::next(&mut b);
    settings::next(&mut b);
    assert_eq!(settings::selected(&b), Some(Setting::AutoCloudSync));
    let out = render(&b, 100, 40);
    assert!(out.contains(Setting::AutoCloudSync.help()), "{out}");
}

#[test]
fn small_panel_scrolls_to_selection() {
    isolate_config();
    let mut b = launcher_browser();
    b.config_index = settings::selectable(&b).len() - 1;
    let out = render(&b, 100, 12);
    assert!(out.contains("Proxy"), "selected row visible\n{out}");
    assert!(out.contains(Setting::Proxy.help()), "help visible\n{out}");
}

#[test]
fn shortcuts_map_to_rows() {
    for (c, s) in [
        ('h', Setting::Hltb),
        ('t', Setting::AutoCloudSync),
        ('l', Setting::StoreLanguage),
        ('b', Setting::Binary),
        ('o', Setting::Presence),
        ('w', Setting::PersonaWait),
        ('g', Setting::CloudSync),
        ('d', Setting::DownloadLimit),
        ('s', Setting::Shaders),
        ('a', Setting::AntiCheat),
        ('e', Setting::Proxy),
    ] {
        assert_eq!(settings::from_shortcut(c), Some(s));
    }
    assert_eq!(settings::from_shortcut('j'), None);
    assert_eq!(settings::from_shortcut('k'), None);
    assert_eq!(Setting::StoreLanguage.kind(), Kind::Cycle);
    assert_eq!(Setting::Proxy.kind(), Kind::Input);
    assert_eq!(Setting::Hltb.kind(), Kind::Toggle);
}

#[test]
fn enter_toggles_and_opens_inputs() {
    isolate_config();
    let mut config = Config::default();
    config.save().unwrap();
    let mut b = launcher_browser();
    let before = config.hltb_enabled;
    settings::activate(&mut b, &mut config);
    assert_eq!(config.hltb_enabled, !before);
    assert_eq!(Config::cached().hltb_enabled, !before);

    settings::apply(Setting::Proxy, &mut b, &mut config);
    assert!(b.config_proxy_input.is_some(), "proxy input opens");
    assert_eq!(
        settings::selected(&b),
        Some(Setting::Proxy),
        "highlight follows"
    );
    b.cancel_proxy_edit();

    settings::apply(Setting::DownloadLimit, &mut b, &mut config);
    assert!(b.download_limit.input.is_some());
    b.cancel_limit_edit();

    settings::apply(Setting::Binary, &mut b, &mut config);
    assert!(b.config_binary_input.is_some());
    b.close_config();
    assert!(b.config_binary_input.is_none(), "close drops input");
}

fn game(id: i32) -> Game {
    use aurelia_tui::interface::aurelia::LibraryGameJson;
    Game::from_library(
        serde_json::from_value::<LibraryGameJson>(serde_json::json!({
            "app_id": id, "name": "Alpha", "is_installed": true
        }))
        .unwrap(),
    )
}

#[test]
fn actions_menu_reaches_app_screens() {
    isolate_config();
    let b = Browser::new(vec![game(10)]);
    let rows = b.action_candidates();
    for (action, key) in [
        (Action::Settings, "p"),
        (Action::Account, "A"),
        (Action::Wallet, "w"),
        (Action::Running, "R"),
        (Action::Friends, "F"),
        (Action::Market, "m"),
        (Action::MarketSearch, "S"),
        (Action::Refresh, "r"),
        (Action::Help, "?"),
        (Action::InstallQueue, "B"),
        (Action::Proton, "P"),
    ] {
        let row = rows.iter().find(|r| r.action == action).unwrap();
        assert_eq!(row.key, key, "{}", row.label);
    }
    assert!(rows.iter().any(|r| r.action == Action::OwnProfile));
}

#[test]
fn account_logout_needs_confirmation_text() {
    isolate_config();
    let mut b = Browser::new(vec![]);
    let draw = |b: &Browser| {
        let mut t = Terminal::new(TestBackend::new(80, 12)).unwrap();
        t.draw(|f| f.render_widget(ui::account::account(b), Rect::new(0, 0, 80, 12)))
            .unwrap();
        t.backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol.clone())
            .collect::<String>()
    };
    assert!(draw(&b).contains("[o] log out"));
    b.confirm_logout = true;
    assert!(draw(&b).contains("[y] yes"));
}
