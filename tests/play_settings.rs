//! Play settings parsing, status and rendering.

use aurelia_tui::browse::{Action, Browser, ROW_LAUNCH_OPTIONS, ROW_PREFIX};
use aurelia_tui::interface::aurelia::{ConfigJson, GameConfigJson, LibraryGameJson, PlayOpts};
use aurelia_tui::interface::game::Game;
use aurelia_tui::interface::play_settings::{self as ps, AntiCheat};
use aurelia_tui::ui;
use tui::Terminal;
use tui::backend::TestBackend;
use tui::layout::Rect;

fn fixture(name: &str) -> String {
    let path = format!(
        "{}/tests/fixtures/play_settings/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(path).unwrap()
}

fn isolate_config() {
    let dir = std::env::temp_dir().join("aurelia-tui-test-config");
    // SAFETY: set before Config::cached().
    unsafe {
        std::env::set_var("AURELIA_TUI_DIR", &dir);
        std::env::set_var("AURELIA_TUI_CACHE_DIR", &dir);
    }
}

fn game(id: u32, installed: bool) -> Game {
    Game::from_library(LibraryGameJson {
        app_id: id,
        name: "Elden".to_string(),
        is_installed: installed,
        install_path: None,
        update_available: false,
        is_owned: true,
        is_family_shared: false,
        platform: None,
        active_branch: None,
        assets: None,
        store_url: None,
        library: Default::default(),
    })
}

fn render<F: FnOnce(&mut tui::Frame<TestBackend>)>(w: u16, h: u16, draw: F) -> String {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| draw(f)).unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol.clone())
        .collect()
}

#[test]
fn config_game_parses_launch_fields() {
    let cfg: GameConfigJson = serde_json::from_str(&fixture("config_game.json")).unwrap();
    assert_eq!(cfg.forced_proton_version.as_deref(), Some("GE-Proton9-20"));
    assert_eq!(
        cfg.launch.launch_options,
        "PROTON_LOG=1 nice -n 5 %command% -windowed"
    );
    assert_eq!(
        cfg.launch.prefix_dir.as_deref(),
        Some("/home/user/Games/Elden")
    );
    assert_eq!(
        cfg.launch.wine_prefix.as_deref(),
        Some("/home/user/Games/Elden/pfx")
    );

    let plain: GameConfigJson = serde_json::from_str(&fixture("config_game_default.json")).unwrap();
    assert!(plain.launch.launch_options.is_empty());
    assert!(plain.launch.prefix_dir.is_none());
    assert!(
        plain
            .launch
            .wine_prefix
            .unwrap()
            .ends_with("compatdata/620/pfx")
    );
}

#[test]
fn config_show_parses_play_fields() {
    let cfg: ConfigJson = serde_json::from_str(&fixture("config_show.json")).unwrap();
    assert_eq!(cfg.proton_version.as_deref(), Some("GE-Proton9-20"));
    assert!(!cfg.play.shaders_auto());
    assert!(!cfg.play.anticheat_auto_install);

    // Older configs omit both keys.
    let old: ConfigJson = serde_json::from_str("{}").unwrap();
    assert!(old.play.shaders_auto());
    assert!(old.play.anticheat_auto_install);
    assert!(ConfigJson::default().play.anticheat_auto_install);
}

#[test]
fn shader_events_map_to_status() {
    let lines: Vec<Option<String>> = fixture("shaders_stderr.ndjson")
        .lines()
        .map(ps::shader_event_status)
        .collect();
    assert_eq!(lines[0].as_deref(), Some("shaders: starting (2 caches)"));
    assert_eq!(lines[1].as_deref(), Some("shaders 12.5% (120/960)"));
    assert_eq!(lines[2].as_deref(), Some("shaders: finishing"));
    assert_eq!(ps::shader_event_status("not json"), None);
    assert_eq!(ps::shader_event_status(r#"{"event":"progress"}"#), None);

    assert_eq!(
        ps::shader_result_status(&fixture("shaders_result.json"), true),
        "Shaders processed (960/960)"
    );
    assert_eq!(
        ps::shader_result_status(&fixture("shaders_skipped.json"), true),
        "Shaders skipped: shaders already processed"
    );
    let err = ps::shader_result_status(
        r#"{"error":"Elden is not installed","type":"not_found"}"#,
        false,
    );
    assert_eq!(err, "Shaders failed: Elden is not installed");
    assert_eq!(ps::shader_result_status("", false), "Shaders failed");
}

#[test]
fn anticheat_events_map_to_status() {
    let line = fixture("anticheat_progress.ndjson");
    assert_eq!(
        ps::anticheat_event_status(AntiCheat::Eac, line.trim()).as_deref(),
        Some("EAC runtime downloading 25.0%")
    );
    assert_eq!(
        ps::anticheat_result_status(AntiCheat::Eac, &fixture("anticheat_result.json"), true),
        "EAC runtime already installed"
    );
    assert_eq!(
        ps::anticheat_result_status(
            AntiCheat::BattlEye,
            r#"{"name":"Proton BattlEye Runtime","status":"installed"}"#,
            true
        ),
        "BattlEye runtime installed"
    );
    assert!(
        ps::anticheat_result_status(
            AntiCheat::Eac,
            r#"{"error":"not logged in","type":"auth"}"#,
            false
        )
        .starts_with("EAC runtime failed: not logged in")
    );
    assert_eq!(AntiCheat::Eac.arg(), "eac");
    assert_eq!(AntiCheat::BattlEye.arg(), "battleye");
}

#[test]
fn launch_options_error_fixture_is_aurelia_shape() {
    let v: serde_json::Value = serde_json::from_str(&fixture("error_launch_options.json")).unwrap();
    let msg = v["error"].as_str().unwrap();
    assert!(msg.starts_with("invalid --launch-options"));
}

#[test]
fn prefix_tilde_expands() {
    let home = std::env::var("HOME").unwrap();
    assert_eq!(ps::expand_prefix("~/Games/x"), format!("{home}/Games/x"));
    assert_eq!(ps::expand_prefix(" /abs/p "), "/abs/p");
    assert_eq!(ps::expand_prefix("   "), "");
}

#[test]
fn play_opts_skip_shaders_defaults_off() {
    assert!(!PlayOpts::default().skip_shaders);
}

#[test]
fn shader_actions_need_install() {
    isolate_config();
    let b = Browser::new(vec![game(1, true)]);
    let acts: Vec<Action> = b.action_candidates().iter().map(|r| r.action).collect();
    assert!(acts.contains(&Action::PlaySkipShaders));
    assert!(acts.contains(&Action::ProcessShaders));
    assert!(acts.contains(&Action::ProcessShadersForce));

    let b = Browser::new(vec![game(2, false)]);
    let acts: Vec<Action> = b.action_candidates().iter().map(|r| r.action).collect();
    assert!(!acts.contains(&Action::PlaySkipShaders));
    assert!(!acts.contains(&Action::ProcessShaders));
}

#[test]
fn game_settings_render_launch_rows() {
    isolate_config();
    let mut b = Browser::new(vec![game(1, true)]);
    b.show_game_config = true;
    b.game_config = Some(serde_json::from_str(&fixture("config_game.json")).unwrap());
    let text = render(90, 16, |f| {
        f.render_widget(ui::game_config::game_config(&b), Rect::new(0, 0, 90, 16));
    });
    assert!(text.contains("Launch options: PROTON_LOG=1"));
    assert!(text.contains("Wine prefix: /home/user/Games/Elden"));
    assert!(text.contains("Prefix in use: /home/user/Games/Elden/pfx (custom)"));
    assert_eq!(Browser::GAME_CONFIG_ROWS, ROW_PREFIX + 1);
}

#[test]
fn game_settings_edit_input_round_trip() {
    isolate_config();
    let mut b = Browser::new(vec![game(1, true)]);
    b.show_game_config = true;
    b.game_config = Some(serde_json::from_str(&fixture("config_game_default.json")).unwrap());
    // Walk to the launch options row.
    while b.game_config_index != ROW_LAUNCH_OPTIONS {
        b.game_config_next();
    }
    // Set input directly; activate shells out.
    b.game_config_input = Some(String::new());
    for c in "-dx11".chars() {
        b.game_config_input_push(c);
    }
    assert!(b.game_config_editing());
    let text = render(90, 16, |f| {
        f.render_widget(ui::game_config::game_config(&b), Rect::new(0, 0, 90, 16));
    });
    assert!(text.contains("Launch options: -dx11_"));
    assert!(text.contains("empty clears"));
    b.game_config_input_pop();
    assert_eq!(b.game_config_input.as_deref(), Some("-dx1"));
    b.game_config_input_cancel();
    assert!(!b.game_config_editing());
}

#[test]
fn settings_render_play_rows() {
    isolate_config();
    let mut b = Browser::new(vec![]);
    b.show_config = true;
    b.config_info = Some(serde_json::from_str(&fixture("config_show.json")).unwrap());
    let rows = ui::play_settings::settings_rows(&b);
    assert_eq!(rows[0], ("Shader pre-caching", "off".to_string()));
    assert_eq!(rows[1], ("Anti-cheat auto-install", "off".to_string()));
    let text = render(140, 24, |f| {
        f.render_widget(ui::config::config(&b), Rect::new(0, 0, 140, 24));
    });
    assert!(text.contains("Shader pre-caching"));
    assert!(text.contains("[s] shaders"));
}
