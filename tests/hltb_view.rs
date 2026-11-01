//! Exercise the persisted switch, model guards, and rendered UI in one scenario.
//! A single test keeps this suite's process-wide config cache and environment
//! overrides from racing against other preference changes.
use aurelia_tui::browse::{Action, Browser};
use aurelia_tui::interface::{aurelia::LibraryGameJson, game::Game, hltb::HltbState};
use aurelia_tui::{config::Config, ui};
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tui::{Terminal, backend::TestBackend};

fn isolate_config() -> PathBuf {
    // A separate test binary avoids races with other suites' config overrides.
    let dir = std::env::temp_dir().join(format!("aurelia-hltb-menu-{}", std::process::id()));
    unsafe {
        std::env::set_var("AURELIA_TUI_DIR", &dir);
        std::env::set_var("AURELIA_TUI_CACHE_DIR", &dir);
        std::env::set_var("AURELIA_BIN", dir.join("missing-backend"));
    }
    dir
}

fn game(id: u32, name: &str, installed: bool) -> Game {
    Game::from_library(LibraryGameJson {
        app_id: id,
        name: name.into(),
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

#[test]
fn hltb_switch_preserves_config_and_controls_queries_rendering_and_actions() {
    let dir = isolate_config();
    let mut config = Config {
        favorite_games: vec![400],
        ..Default::default()
    };
    config.save().unwrap();
    for installed in [true, false] {
        let mut browser = Browser::new(vec![game(400, "Portal", installed)]);
        browser.open_actions();
        for character in "howlongtobeat".chars() {
            browser.actions_filter_push(character);
        }
        assert_eq!(browser.selected_action(), Some(Action::Hltb));
        assert_eq!(browser.filtered_actions()[0].key, ",ct");
        browser.open_hltb();
        assert!(browser.notice.as_deref().unwrap().contains("waiting"));
    }

    // Load a known cache entry before switching off: late/retained results must
    // disappear from the UI even though the per-game snapshot is still Ready.
    fs::create_dir_all(dir.join("hltb")).unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    fs::write(
        dir.join("hltb/400.json"),
        serde_json::json!({
            "version": 1, "steam_id": 400, "fetched_at": now,
            "data": {"game_id": 7230, "game_name": "Portal", "main": 11230,
                     "extra": 18943, "completionist": 37927}
        })
        .to_string(),
    )
    .unwrap();
    let loaded = game(400, "Portal", true);
    loaded.query_hltb();
    let deadline = Instant::now() + Duration::from_secs(2);
    while matches!(loaded.get_hltb(), HltbState::Pending | HltbState::Loading)
        && Instant::now() < deadline
    {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(matches!(loaded.get_hltb(), HltbState::Ready { .. }));
    config.hltb_enabled = false;
    config.save().unwrap();
    assert!(!Config::new().unwrap().hltb_enabled, "off survives reload");
    assert_eq!(Config::cached().favorite_games, [400]);

    let never_requested = game(620, "Portal 2", false);
    never_requested.query_hltb();
    assert!(
        matches!(never_requested.get_hltb(), HltbState::Pending),
        "disabled queries must not spawn a worker"
    );
    let mut browser = Browser::new(vec![loaded.clone()]);
    assert!(
        !browser
            .action_candidates()
            .iter()
            .any(|row| row.action == Action::Hltb)
    );
    browser.open_hltb();
    assert!(browser.notice.as_deref().unwrap().contains("disabled"));
    assert!(
        matches!(loaded.get_hltb(), HltbState::Ready { .. }),
        "retained state is intentionally unchanged"
    );

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal
        .draw(|f| f.render_widget(ui::detail::detail(Some(&loaded), false, 60, 22), f.size()))
        .unwrap();
    assert!(
        !text(&terminal).contains("HLTB"),
        "cached or late results stay hidden"
    );

    assert!(browser.open_config().is_err(), "fixture backend is missing");
    assert!(
        browser.show_config,
        "local settings remain reachable after a backend failure"
    );
    assert!(browser.config_info.is_none());
    terminal
        .draw(|f| f.render_widget(ui::config::config(&browser), f.size()))
        .unwrap();
    let settings = text(&terminal);
    assert!(settings.contains("HowLongToBeat"));
    assert!(!settings.contains("HowLongToBeat [h]"));
    assert!(settings.contains("off"));
    assert!(settings.contains("Launcher"));
    assert!(settings.contains("Unavailable"));

    // Failed persistence must not update the process-wide cached preference.
    let config_path = dir.join("config.json");
    let original = fs::read(&config_path).unwrap();
    fs::remove_file(&config_path).unwrap();
    fs::create_dir(&config_path).unwrap();
    let mut failed = config.clone();
    failed.hltb_enabled = true;
    assert!(failed.save().is_err());
    assert!(!Config::cached().hltb_enabled);
    fs::remove_dir(&config_path).unwrap();
    fs::write(&config_path, original).unwrap();

    config.hltb_enabled = true;
    config.save().unwrap();
    assert!(
        browser
            .action_candidates()
            .iter()
            .any(|row| row.action == Action::Hltb)
    );
    terminal
        .draw(|f| f.render_widget(ui::detail::detail(Some(&loaded), false, 60, 22), f.size()))
        .unwrap();
    assert!(text(&terminal).contains("HLTB"));
    fs::remove_dir_all(dir).unwrap();
}

fn text(terminal: &Terminal<TestBackend>) -> String {
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer.get(x, y).symbol.clone())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
