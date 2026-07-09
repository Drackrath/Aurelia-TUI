//! Exercises the per-game Actions menu model (applicability + filtering) and
//! renders the new 0.1.16/0.1.17 overlays (actions, versions, game settings,
//! collections, runtime plugins) into tui's off-screen `TestBackend` to prove
//! they lay out and paint without panicking. State is set through the public
//! fields directly so the tests never shell out to the `aurelia` binary.

use aurelia_tui::browse::{Action, Browser};
use aurelia_tui::interface::aurelia::{
    AvailableJson, CollectionJson, DepotManifestInfo, GameConfigJson, LibraryGameJson,
    PluginStatusJson,
};
use aurelia_tui::interface::game::Game;
use aurelia_tui::ui;
use std::collections::BTreeMap;
use tui::backend::TestBackend;
use tui::layout::Rect;
use tui::Terminal;

fn isolate_config() {
    let dir = std::env::temp_dir().join("aurelia-tui-test-config");
    // SAFETY: set before any Config::cached() call in this test process.
    unsafe {
        std::env::set_var("AURELIA_TUI_DIR", &dir);
        std::env::set_var("AURELIA_TUI_CACHE_DIR", &dir);
    }
}

fn game(id: u32, name: &str, installed: bool) -> Game {
    Game::from_library(LibraryGameJson {
        app_id: id,
        name: name.to_string(),
        is_installed: installed,
        install_path: None,
        update_available: false,
        is_owned: true,
        is_family_shared: false,
        platform: None,
        active_branch: None,
        assets: None,
        store_url: None,
    })
}

/// Flatten a rendered `TestBackend` buffer into one string for assertions.
fn render<F: FnOnce(&mut tui::Frame<TestBackend>)>(w: u16, h: u16, draw: F) -> String {
    let backend = TestBackend::new(w, h);
    let mut terminal = Terminal::new(backend).unwrap();
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
fn action_candidates_depend_on_install_state() {
    isolate_config();

    // An installed game offers Play/Update/Verify/Uninstall, not Install.
    let mut browser = Browser::new(vec![game(1, "Alpha", true)]);
    let actions: Vec<Action> = browser.action_candidates().iter().map(|r| r.action).collect();
    assert!(actions.contains(&Action::Play), "installed game can Play");
    assert!(actions.contains(&Action::Update), "installed game can Update");
    assert!(actions.contains(&Action::Uninstall), "installed game can Uninstall");
    assert!(actions.contains(&Action::Move), "installed game can Move");
    assert!(!actions.contains(&Action::Install), "installed game hides Install");

    // Every game offers the new feature actions regardless of install state.
    assert!(actions.contains(&Action::Versions), "Versions & pinning listed");
    assert!(actions.contains(&Action::Collections), "Collections listed");
    assert!(actions.contains(&Action::Engine), "Runtime plugins listed");
    assert!(actions.contains(&Action::GameSettings), "Game settings listed");

    // A not-installed game offers Install, not Play/Uninstall/Move.
    browser = Browser::new(vec![game(2, "Beta", false)]);
    let actions: Vec<Action> = browser.action_candidates().iter().map(|r| r.action).collect();
    assert!(actions.contains(&Action::Install), "uninstalled game can Install");
    assert!(!actions.contains(&Action::Play), "uninstalled game hides Play");
    assert!(!actions.contains(&Action::Uninstall), "uninstalled game hides Uninstall");
    assert!(!actions.contains(&Action::Move), "uninstalled game hides Move");
}

#[test]
fn actions_filter_narrows_and_selection_tracks() {
    isolate_config();
    let mut browser = Browser::new(vec![game(1, "Alpha", true)]);
    browser.open_actions();
    let full = browser.filtered_actions().len();
    assert!(full > 5, "menu lists many actions when unfiltered");

    // Typing narrows to the matching rows and resets the highlight.
    for c in "collect".chars() {
        browser.actions_filter_push(c);
    }
    let filtered = browser.filtered_actions();
    assert_eq!(filtered.len(), 1, "only Collections matches 'collect'");
    assert_eq!(filtered[0].action, Action::Collections);
    assert_eq!(browser.selected_action(), Some(Action::Collections));

    browser.actions_filter_pop();
    assert!(
        browser.filtered_actions().len() >= 1,
        "backspacing the filter re-widens the list"
    );
}

#[test]
fn actions_menu_renders_categories() {
    isolate_config();
    let mut browser = Browser::new(vec![game(1, "Alpha", true)]);
    browser.open_actions();
    let text = render(70, 30, |f| {
        f.render_widget(ui::actions::actions(&browser), Rect::new(0, 0, 70, 30));
    });
    assert!(text.contains("Actions"), "menu title renders");
    assert!(text.contains("Alpha"), "menu title names the game");
    assert!(text.contains("Versions"), "a category header renders");
    assert!(text.contains("pinning"), "the versions/pinning action label renders");
}

#[test]
fn versions_overlay_renders_pin_and_manifest() {
    isolate_config();
    let mut browser = Browser::new(vec![game(730, "CS", true)]);
    browser.show_versions = true;
    browser.versions_manifests = vec![DepotManifestInfo {
        depot_id: 731,
        depot_name: Some("CS content".to_string()),
        branch: "public".to_string(),
        manifest_id: 12345,
        size: 1024,
    }];
    let mut pinned = BTreeMap::new();
    pinned.insert("731".to_string(), 12345u64);
    browser.versions_available = Some(AvailableJson {
        app_id: 730,
        available: true,
        install_path: Some("C:/games/cs".to_string()),
        pinned: true,
        pinned_manifests: pinned,
    });
    let text = render(80, 20, |f| {
        f.render_widget(ui::versions::versions(&browser), Rect::new(0, 0, 80, 20));
    });
    assert!(text.contains("PINNED"), "pinned banner renders");
    assert!(text.contains("12345"), "the depot manifest id renders");
    assert!(text.contains("731"), "the depot id renders");
}

#[test]
fn game_settings_overlay_renders_rows() {
    isolate_config();
    let mut browser = Browser::new(vec![game(1, "Alpha", true)]);
    browser.show_game_config = true;
    browser.game_config = Some(GameConfigJson {
        app_id: 1,
        forced_proton_version: Some("GE-Proton9".to_string()),
        platform_preference: Some("windows".to_string()),
        runner: "umu".to_string(),
        launch_script: None,
    });
    let text = render(70, 12, |f| {
        f.render_widget(ui::game_config::game_config(&browser), Rect::new(0, 0, 70, 12));
    });
    assert!(text.contains("Runner"), "runner row renders");
    assert!(text.contains("umu"), "current runner value renders");
    assert!(text.contains("GE-Proton9"), "forced proton value renders");
    assert!(text.contains("Platform"), "platform row renders");
}

#[test]
fn collections_overlay_renders_list() {
    isolate_config();
    let mut browser = Browser::new(vec![game(1, "Alpha", true)]);
    browser.show_collections = true;
    browser.collections = vec![CollectionJson {
        id: "uc-1".to_string(),
        name: "Favourites".to_string(),
        kind: "static".to_string(),
        dynamic: false,
        count: Some(7),
        app_ids: vec![1, 2, 3],
    }];
    let text = render(70, 12, |f| {
        f.render_widget(ui::collections::collections(&browser), Rect::new(0, 0, 70, 12));
    });
    assert!(text.contains("Collections"), "overlay title renders");
    assert!(text.contains("Favourites"), "collection name renders");
    assert!(text.contains("7 games"), "collection count renders");
}

#[test]
fn engine_overlay_renders_plugin_status() {
    isolate_config();
    let mut browser = Browser::new(vec![game(1, "Alpha", true)]);
    browser.show_engine = true;
    browser.engine_umu = Some(PluginStatusJson {
        enabled: true,
        custom_path: None,
        installed: None,
        linux: false,
    });
    let text = render(70, 12, |f| {
        f.render_widget(ui::engine::engine(&browser), Rect::new(0, 0, 70, 12));
    });
    assert!(text.contains("umu-launcher"), "umu row renders");
    assert!(text.contains("luxtorpeda"), "luxtorpeda row renders");
    assert!(text.contains("Steam Linux Runtime"), "steam runtime row renders");
    assert!(text.contains("enabled"), "umu enabled status renders");
}
