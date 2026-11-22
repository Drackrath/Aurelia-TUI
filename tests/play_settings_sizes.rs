//! Play overlays at 120x40 and 80x24.

use aurelia_tui::browse::{Browser, ROW_LAUNCH_OPTIONS};
use aurelia_tui::interface::aurelia::{LibraryGameJson, ProtonJson};
use aurelia_tui::interface::game::Game;
use aurelia_tui::interface::play_settings::{self as ps, AntiCheat};
use aurelia_tui::ui;
use tui::Terminal;
use tui::backend::TestBackend;
use tui::layout::Rect;

const SIZES: [(u16, u16); 2] = [(120, 40), (80, 24)];

fn isolate_config() {
    let dir = std::env::temp_dir().join("aurelia-tui-test-config");
    // SAFETY: set before Config::cached().
    unsafe {
        std::env::set_var("AURELIA_TUI_DIR", &dir);
        std::env::set_var("AURELIA_TUI_CACHE_DIR", &dir);
    }
}

fn game(id: u32) -> Game {
    Game::from_library(LibraryGameJson {
        app_id: id,
        name: "Baldur's Gate 3".to_string(),
        is_installed: true,
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

fn proton(name: &str, installed: bool) -> ProtonJson {
    ProtonJson {
        name: name.to_string(),
        label: String::new(),
        size: 0,
        installed,
        is_default: false,
    }
}

/// Rendered rows of the buffer.
fn lines<F: FnOnce(&mut tui::Frame<TestBackend>)>(w: u16, h: u16, draw: F) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| draw(f)).unwrap();
    let buf = terminal.backend().buffer();
    (0..h)
        .map(|y| (0..w).map(|x| buf.get(x, y).symbol.clone()).collect())
        .collect()
}

fn has(rows: &[String], needle: &str) -> bool {
    rows.iter().any(|r| r.contains(needle))
}

fn settings_browser() -> Browser {
    let mut b = Browser::new(vec![game(1086940)]);
    b.show_game_config = true;
    b.game_config = Some(
        serde_json::from_value(serde_json::json!({
            "app_id": 1086940, "runner": "auto",
            "launch_options": "", "prefix_dir": null,
            "wine_prefix": "/home/user/.steam/steam/steamapps/compatdata/1086940/pfx"
        }))
        .unwrap(),
    );
    b
}

#[test]
fn game_settings_last_row_and_hints_fit() {
    isolate_config();
    for (w, h) in SIZES {
        let mut b = settings_browser();
        for _ in 1..Browser::GAME_CONFIG_ROWS {
            b.game_config_next();
        }
        let area = ui::centered_rect(58, 60, Rect::new(0, 0, w, h));
        let rows = lines(w, h, |f| {
            f.render_widget(ui::game_config::game_config(&b, area), area)
        });
        assert!(has(&rows, "▶ Aurelia Steam Cloud sync"), "last row visible at {w}x{h}");
        assert!(has(&rows, "[Esc] close"), "hint fits at {w}x{h}");
        assert!(has(&rows, "[x] del script"), "hint fits at {w}x{h}");
        assert!(has(&rows, "Prefix in use:"), "prefix line at {w}x{h}");
        assert!(has(&rows, "1086940/pfx"), "prefix tail kept at {w}x{h}");
    }
}

#[test]
fn launch_options_hint_fits() {
    isolate_config();
    for (w, h) in SIZES {
        let mut b = settings_browser();
        while b.game_config_index != ROW_LAUNCH_OPTIONS {
            b.game_config_next();
        }
        b.game_config_input = Some("-dx11".into());
        let area = ui::centered_rect(58, 60, Rect::new(0, 0, w, h));
        let rows = lines(w, h, |f| {
            f.render_widget(ui::game_config::game_config(&b, area), area)
        });
        assert!(has(&rows, "▶ Launch options: -dx11_"), "row at {w}x{h}");
        assert!(
            has(&rows, "e.g. VAR=1 %command% -arg"),
            "example fits at {w}x{h}"
        );
        assert!(has(&rows, "[Esc] cancel"), "keys fit at {w}x{h}");
    }
}

#[test]
fn choice_picker_lists_and_scrolls() {
    isolate_config();
    for (w, h) in SIZES {
        let mut b = settings_browser();
        b.game_config_protons = (0..30).map(|i| format!("GE-Proton10-{i}")).collect();
        b.game_config_index = 1;
        b.game_config_activate();
        assert!(b.game_config_picking(), "Enter opens a picker");
        let picker = b.play.picker.clone().unwrap();
        assert_eq!(picker.options.len(), 31);
        assert_eq!(picker.current, 0, "global default is current");
        for _ in 0..30 {
            b.picker_next();
        }
        let area = ui::centered_rect(40, 50, Rect::new(0, 0, w, h));
        let rows = lines(w, h, |f| {
            f.render_widget(ui::play_settings::picker(&b, area), area)
        });
        assert!(
            has(&rows, "▶ GE-Proton10-29"),
            "last option visible at {w}x{h}"
        );
        assert!(has(&rows, "[Enter] apply"), "apply hint at {w}x{h}");
        assert!(has(&rows, "[Esc] cancel"), "cancel hint at {w}x{h}");
        b.picker_cancel();
        assert!(!b.game_config_picking());
    }
}

#[test]
fn picker_marks_current_value() {
    isolate_config();
    let mut b = settings_browser();
    b.game_config_index = 3;
    b.game_config_activate();
    let p = b.play.picker.clone().unwrap();
    let labels: Vec<&str> = p.options.iter().map(|(l, _)| l.as_str()).collect();
    assert_eq!(labels, ["auto", "on", "off"]);
    let area = Rect::new(0, 0, 40, 10);
    let rows = lines(40, 10, |f| {
        f.render_widget(ui::play_settings::picker(&b, area), area)
    });
    assert!(has(&rows, "auto  [current]"));
}

#[test]
fn proton_overlay_scrolls_and_shows_default() {
    isolate_config();
    for (w, h) in SIZES {
        let mut b = Browser::new(vec![game(1)]);
        b.show_proton = true;
        b.protons = (0..40)
            .map(|i| proton(&format!("GE-Proton9-{i}"), i == 3))
            .collect();
        b.protons[3].is_default = true;
        b.play.proton_default = "GE-Proton-latest".into();
        b.play.proton_default_row = Some("GE-Proton9-3".into());
        b.proton_index = 39;
        let area = ui::centered_rect(60, 70, Rect::new(0, 0, w, h));
        let rows = lines(w, h, |f| {
            f.render_widget(ui::proton::proton(&b, area), area)
        });
        assert!(has(&rows, "▶ GE-Proton9-39"), "last row visible at {w}x{h}");
        assert!(has(&rows, "Default: GE-Proton-latest → GE-Proton9-3"));
        for key in [
            "[d] default",
            "[i] install",
            "[u] remove",
            "[e] EAC",
            "[b] BattlEye",
            "[Esc] close",
        ] {
            assert!(has(&rows, key), "{key} shown at {w}x{h}");
        }
    }
}

#[test]
fn play_confirms_render_and_cancel() {
    isolate_config();
    for (w, h) in SIZES {
        let mut b = Browser::new(vec![game(7)]);
        b.ask_shaders(7, false);
        let area = ui::centered_rect(50, 25, Rect::new(0, 0, w, h));
        let prompt = ui::play_settings::confirm_prompt(&b).unwrap();
        let rows = lines(w, h, |f| f.render_widget(prompt, area));
        assert!(has(&rows, "Vulkan"), "question shown at {w}x{h}");
        assert!(has(&rows, "[y]"), "confirm key at {w}x{h}");
        b.answer_play_confirm(false);
        assert!(b.play_confirm_prompt().is_none(), "n cancels");

        b.ask_anticheat(AntiCheat::BattlEye);
        assert!(b.play_confirm_prompt().unwrap().contains("BattlEye"));
        b.answer_play_confirm(false);
        assert!(b.play.confirm_anticheat.is_none());
    }
}

#[test]
fn protons_sort_naturally() {
    let names = [
        "Proton - Experimental",
        "Proton 9.0",
        "Proton 5.13",
        "Proton 6.3",
        "GE-Proton11-12",
        "GE-Proton11-5",
        "GE-Proton8-26",
        "GE-Proton8-27-LoL",
        "GE-Proton8-26-LoL",
        "GE-Proton8-25",
        "GE-Proton11-9",
    ];
    let mut list: Vec<ProtonJson> = names.iter().map(|n| proton(n, false)).collect();
    ps::natural_sort_protons(&mut list);
    let got: Vec<&str> = list.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        got,
        [
            "Proton - Experimental",
            "Proton 9.0",
            "Proton 6.3",
            "Proton 5.13",
            "GE-Proton11-12",
            "GE-Proton11-9",
            "GE-Proton11-5",
            "GE-Proton8-26",
            "GE-Proton8-25",
            "GE-Proton8-27-LoL",
            "GE-Proton8-26-LoL",
        ]
    );
}

#[test]
fn default_resolves_through_symlink() {
    let names: Vec<String> = ["GE-Proton11-11", "Proton - Experimental"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(
        ps::resolve_default("ge-proton11-11", &names, &[]).as_deref(),
        Some("GE-Proton11-11")
    );
    assert_eq!(
        ps::resolve_default("experimental", &names, &[]).as_deref(),
        Some("Proton - Experimental")
    );
    // Scratch dir under cargo's tmp.
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("ps-symlink");
    std::fs::create_dir_all(dir.join("GE-Proton11-11")).unwrap();
    let link = dir.join("GE-Proton-latest");
    if std::fs::symlink_metadata(&link).is_err() {
        std::os::unix::fs::symlink(dir.join("GE-Proton11-11"), &link).unwrap();
    }
    assert_eq!(
        ps::resolve_default("GE-Proton-latest", &names, std::slice::from_ref(&dir)).as_deref(),
        Some("GE-Proton11-11")
    );
    assert_eq!(ps::resolve_default("GE-Proton-latest", &names, &[]), None);
}
