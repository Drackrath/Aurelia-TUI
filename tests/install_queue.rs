//! Install queue, progress, errors, download limit.

use aurelia_tui::browse::install_queue::entry_status;
use aurelia_tui::browse::{Browser, InstallPhase};
use aurelia_tui::interface::aurelia::LibraryGameJson;
use aurelia_tui::interface::game::Game;
use aurelia_tui::interface::install_queue::{self as iq, ControlOutcome, ErrorKind, QueueEntry};
use aurelia_tui::ui;
use tui::Terminal;
use tui::backend::TestBackend;
use tui::layout::Rect;

fn fixture(name: &str) -> String {
    let path = format!(
        "{}/tests/fixtures/install_queue/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(path).unwrap()
}

fn json(name: &str) -> serde_json::Value {
    iq::parse_stdout(&fixture(name)).unwrap()
}

fn entries() -> Vec<QueueEntry> {
    iq::parse_list(json("install_list.json")).unwrap()
}

fn isolate_config() {
    let dir = std::env::temp_dir().join("aurelia-tui-test-config");
    // SAFETY: set before config reads.
    unsafe {
        std::env::set_var("AURELIA_TUI_DIR", &dir);
        std::env::set_var("AURELIA_TUI_CACHE_DIR", &dir);
    }
}

fn game(id: u32, name: &str) -> Game {
    Game::from_library(LibraryGameJson {
        app_id: id,
        name: name.to_string(),
        is_installed: false,
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

fn state_of(browser: &Browser, id: i32) -> String {
    let game = browser
        .visible()
        .into_iter()
        .find(|g| g.id == id)
        .unwrap()
        .clone();
    game.get_status().map(|s| s.state).unwrap_or_default()
}

#[test]
fn list_parses_running_and_backlog_rows() {
    let list = entries();
    assert_eq!(list.len(), 4);
    assert!(list[0].is_running());
    assert_eq!(list[0].library, None, "running rows lack backlog fields");
    assert!(list[1].is_resumable());
    assert_eq!(
        list[1].library.as_deref(),
        Some("/home/user/.local/share/Steam")
    );
    assert_eq!(list[2].error_type.as_deref(), Some("disk_full"));
    assert_eq!(list[2].note(), "Disk full");
    assert!(!list[3].is_resumable(), "aborted is not auto-resumable");
    assert_eq!(list[3].label(), "app 70");
    assert_eq!(list[3].progress_percent(), 0.0);
}

#[test]
fn entry_status_marks_resumable_as_paused() {
    let list = entries();
    assert_eq!(entry_status(&list[0]).unwrap(), "downloading 50.0%");
    assert_eq!(entry_status(&list[1]).unwrap(), "paused 25.0%");
    assert_eq!(entry_status(&list[2]).unwrap(), "paused 25.0% (disk full)");
    assert_eq!(entry_status(&list[3]), None);
}

#[test]
fn restart_adopts_backlog_into_games() {
    isolate_config();
    let mut browser = Browser::new(vec![
        game(220, "Half-Life 2"),
        game(400, "Portal"),
        game(620, "Portal 2"),
        game(70, "Half-Life"),
    ]);
    browser.apply_queue(entries());
    assert_eq!(state_of(&browser, 220), "downloading 50.0%");
    assert_eq!(state_of(&browser, 400), "paused 25.0%");
    assert!(state_of(&browser, 620).starts_with("paused 25.0%"));
    assert_eq!(state_of(&browser, 70), "uninstalled", "aborted stays idle");

    let phase = |b: &Browser, id: i32| {
        let g = b
            .visible()
            .into_iter()
            .find(|g| g.id == id)
            .unwrap()
            .clone();
        b.install_phase(&g)
    };
    assert_eq!(phase(&browser, 220), InstallPhase::Active);
    assert_eq!(phase(&browser, 400), InstallPhase::Paused);
    assert_eq!(phase(&browser, 620), InstallPhase::Paused);
    assert_eq!(phase(&browser, 70), InstallPhase::Idle);
    assert_eq!(
        browser.install_library_for(620).as_deref(),
        Some("/mnt/games")
    );

    // Daemon pauses the adopted install.
    let mut next = entries();
    next[0].state = "paused".into();
    browser.apply_queue(next);
    assert_eq!(state_of(&browser, 220), "paused 50.0%");
    assert_eq!(phase(&browser, 220), InstallPhase::Paused);
}

#[test]
fn adoption_keeps_live_statuses() {
    isolate_config();
    let browser_game = game(400, "Portal");
    let mut browser = Browser::new(vec![browser_game.clone()]);
    browser_game
        .clone()
        .update_status(aurelia_tui::interface::game_status::GameStatus {
            state: "downloading 80.0%".into(),
            installdir: String::new(),
            size: 0.0,
        });
    browser.apply_queue(entries());
    assert_eq!(state_of(&browser, 400), "downloading 80.0%");
}

#[test]
fn progress_uses_reused_bytes_without_double_count() {
    let text = fixture("progress_resume.ndjson");
    let events: Vec<_> = text.lines().filter_map(iq::parse_progress).collect();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].reused_bytes, 1_000_000_000);
    assert_eq!(
        iq::progress_status(&events[0]),
        "verifying 25.0% · 1.0 GB reused"
    );
    assert_eq!(
        iq::progress_status(&events[1]),
        "downloading 75.0% · ETA 1m 20s · 12.5 MB/s · 1.0 GB reused"
    );
    // Overshoot from old builds clamps.
    let mut over = events[1].clone();
    over.bytes_downloaded = 5_000_000_000;
    assert!(iq::progress_status(&over).starts_with("downloading 100.0%"));
    assert_eq!(iq::parse_progress(&fixture("result_installed.json")), None);
}

#[test]
fn typed_errors_read_clearly() {
    let disk = iq::parse_error(&json("error_disk_full.json")).unwrap();
    assert_eq!(disk.kind, ErrorKind::DiskFull);
    assert!(disk.describe().starts_with("Disk full: not enough space"));
    assert!(disk.describe().contains("pick another library"));

    let offline = iq::parse_error(&json("error_offline.json")).unwrap();
    assert_eq!(offline.kind, ErrorKind::Offline);
    assert!(offline.describe().starts_with("Offline: "));

    let stopped = iq::parse_error(&json("error_stopped.json")).unwrap();
    assert_eq!(stopped.kind, ErrorKind::Cancelled);
    assert!(stopped.describe().contains("stays in the install queue"));

    assert_eq!(
        ErrorKind::parse("steam_config_mismatch"),
        ErrorKind::SteamConfigMismatch
    );
    assert_eq!(
        ErrorKind::parse("network_timeout"),
        ErrorKind::NetworkTimeout
    );
    assert_eq!(ErrorKind::parse("something_new"), ErrorKind::Unknown);
}

#[test]
fn result_status_maps_outcomes() {
    assert_eq!(
        iq::result_status(&fixture("result_installed.json")).as_deref(),
        Some("Installed!")
    );
    let disk = iq::result_status(&fixture("error_disk_full.json")).unwrap();
    assert!(disk.starts_with("Failed: Disk full"));
    let offline = iq::result_status(&fixture("error_offline.json")).unwrap();
    assert!(offline.starts_with("Failed: Offline"));
    assert_eq!(iq::result_status(&fixture("error_paused.json")), None);
    let stopped = iq::result_status(&fixture("error_stopped.json")).unwrap();
    assert!(stopped.starts_with("Failed: Cancelled"));
}

#[test]
fn control_replies_classify() {
    assert_eq!(
        iq::parse_control(&json("pausing.json")),
        ControlOutcome::Signalled
    );
    assert_eq!(
        iq::parse_control(&json("not_found.json")),
        ControlOutcome::NotFound
    );
    assert_eq!(
        iq::parse_control(&json("marked_paused.json")),
        ControlOutcome::Marked("paused".into())
    );
    assert_eq!(
        iq::parse_control(&json("forgotten.json")),
        ControlOutcome::Forgotten(Some("/mnt/games/steamapps/common/Portal 2".into()))
    );
    let nf = iq::parse_error(&json("error_not_found.json")).unwrap();
    assert_eq!(nf.kind, ErrorKind::NotFound);
}

#[test]
fn download_limit_parses_and_describes() {
    assert_eq!(
        iq::parse_limit_json(&json("download_limit.json")),
        Some(8000)
    );
    assert_eq!(iq::describe_limit(8000), "8000 kbit/s (≈ 1.0 MB/s)");
    assert_eq!(iq::describe_limit(800), "800 kbit/s (≈ 100.0 kB/s)");
    assert_eq!(iq::describe_limit(0), "off (unlimited)");
    assert_eq!(iq::parse_limit("off"), Ok(0));
    assert_eq!(iq::parse_limit(" 0 "), Ok(0));
    assert_eq!(iq::parse_limit(""), Ok(0));
    assert_eq!(iq::parse_limit("16000"), Ok(16000));
    assert!(iq::parse_limit("fast").is_err());
}

fn screen(browser: &Browser, w: u16, h: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal
        .draw(|f| {
            let area = ui::centered_rect(72, 70, f.size());
            ui::install_queue::render(f, area, browser);
        })
        .unwrap();
    let buf = terminal.backend().buffer().clone();
    (0..h)
        .map(|y| (0..w).map(|x| buf.get(x, y).symbol.clone()).collect())
        .collect()
}

fn many_entries() -> Vec<QueueEntry> {
    (0..30)
        .map(|i| QueueEntry {
            app_id: 1000 + i,
            name: format!("Queued Game {i}"),
            downloaded_bytes: 3_900_000_000,
            total_bytes: 14_000_000_000,
            state: "paused".into(),
            ..Default::default()
        })
        .collect()
}

#[test]
fn overlay_scrolls_and_hints_fit() {
    isolate_config();
    for (w, h) in [(80, 24), (120, 40)] {
        let mut browser = Browser::new(vec![]);
        browser.apply_queue(many_entries());
        for _ in 0..40 {
            browser.install_queue_next();
        }
        let text = screen(&browser, w, h).join("\n");
        assert!(text.contains("Install queue"), "{w}x{h}");
        assert!(text.contains("▶ Queued Game 29"), "{w}x{h}: last row shown");
        assert!(!text.contains("Queued Game 0 "), "{w}x{h}: scrolled");
        for hint in ui::install_queue::HINTS {
            assert!(text.contains(hint), "{w}x{h}: hint {hint}");
        }
        assert!(text.contains("3.9 GB / 14.0 GB (27.9%)"), "{w}x{h}");
    }
}

#[test]
fn forget_and_stop_ask_first() {
    isolate_config();
    let mut browser = Browser::new(vec![]);
    let mut list = entries();
    list.swap(0, 1);
    browser.apply_queue(list);
    // Portal is paused and selected.
    browser.queue_ask_forget(false);
    assert_eq!(
        browser.queue_prompt().unwrap(),
        "Forget Portal? Downloaded files stay on disk."
    );
    for (w, h) in [(80, 24), (120, 40)] {
        let text = screen(&browser, w, h).join("");
        assert!(text.contains("stay on disk."), "{w}x{h}");
        assert!(text.contains("[y] confirm   [n] cancel"), "{w}x{h}");
    }
    browser.queue_answer(false);
    assert_eq!(browser.install_queue.pending, None);
    assert_eq!(browser.install_queue.entries.len(), 4, "nothing forgotten");

    browser.queue_ask_forget(true);
    assert_eq!(
        browser.queue_prompt().unwrap(),
        "Forget Portal and delete 1.0 GB of partial files?"
    );
    for (w, h) in [(80, 24), (120, 40)] {
        let text = screen(&browser, w, h).join("");
        assert!(text.contains("partial files?"), "{w}x{h}");
        assert!(text.contains("[y] confirm   [n] cancel"), "{w}x{h}");
    }
    browser.queue_answer(false);

    browser.queue_ask_stop();
    assert_eq!(
        browser.queue_prompt().unwrap(),
        "Stop Portal? It stays in the queue as aborted."
    );
    for (w, h) in [(80, 24), (120, 40)] {
        let text = screen(&browser, w, h).join("");
        assert!(text.contains("aborted."), "{w}x{h}");
        assert!(text.contains("[y] confirm   [n] cancel"), "{w}x{h}");
    }
    browser.queue_answer(false);
    assert_eq!(browser.install_queue.status, "Cancelled");

    // Running entries can't be forgotten.
    browser.install_queue_next();
    browser.install_queue_previous();
    browser.install_queue.index = 1;
    browser.queue_ask_forget(false);
    assert_eq!(browser.install_queue.pending, None);
}

#[test]
fn interrupted_game_fits_main_list_at_80() {
    isolate_config();
    let mut browser = Browser::new(vec![game(620, "Portal 2")]);
    browser.apply_queue(entries());
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal
        .draw(|f| {
            let list = ui::list::list(&browser, 32, 0);
            f.render_stateful_widget(list, Rect::new(0, 3, 32, 19), &mut browser.state);
            let selected = browser.selected();
            f.render_widget(
                ui::detail::detail(selected.as_ref(), false, 45, 17),
                Rect::new(32, 3, 48, 19),
            );
        })
        .unwrap();
    let buf = terminal.backend().buffer().clone();
    let text: String = (0..24)
        .map(|y| {
            (0..80)
                .map(|x| buf.get(x, y).symbol.clone())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("Portal 2"));
    assert!(text.contains("25.0%"), "list badge note");
    assert!(
        text.contains("paused 25.0% (disk full)"),
        "detail state fits"
    );
}

#[test]
fn settings_limit_value_renders() {
    isolate_config();
    let mut browser = Browser::new(vec![]);
    browser.download_limit.kbit = Some(8000);
    assert_eq!(
        ui::install_queue::limit_value(&browser),
        "8000 kbit/s (≈ 1.0 MB/s)"
    );
    browser.begin_limit_edit();
    browser.limit_input_pop();
    assert!(ui::install_queue::limit_value(&browser).starts_with("800_  → 100.0 kB/s"));
}

#[test]
fn adoption_seeds_backlog_platform() {
    isolate_config();
    let mut browser = Browser::new(vec![game(620, "Portal 2")]);
    browser.apply_queue(entries());
    let opts = browser.install_opts_for(620);
    assert_eq!(opts.platform.as_deref(), Some("linux"));
    assert_eq!(opts.args(), vec!["-p".to_string(), "linux".to_string()]);
}
