//! Renders "Play as…" launch extras.

use aurelia_tui::browse::Browser;
use aurelia_tui::interface::aurelia::LaunchTargetJson;
use aurelia_tui::interface::launch;
use aurelia_tui::ui;
use tui::Terminal;
use tui::backend::TestBackend;

fn isolate_config() {
    let dir = std::env::temp_dir().join("aurelia-tui-test-launch");
    // SAFETY: set before any config read.
    unsafe {
        std::env::set_var("AURELIA_TUI_DIR", &dir);
        std::env::set_var("AURELIA_TUI_CACHE_DIR", &dir);
    }
}

fn targets() -> Vec<LaunchTargetJson> {
    let raw = include_str!("fixtures/launch/launch_targets.json");
    launch::parse_launch_targets(serde_json::from_str(raw).unwrap()).unwrap()
}

const SIZES: [(u16, u16); 2] = [(120, 40), (80, 24)];

/// Render rows of the frame.
fn render(browser: &Browser, w: u16, h: u16) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal
        .draw(|f| {
            let area = ui::launch_targets::area(f.size());
            f.render_widget(ui::launch_targets::launch_targets(browser, area), area);
        })
        .unwrap();
    let buf = terminal.backend().buffer();
    (0..h)
        .map(|y| (0..w).map(|x| buf.get(x, y).symbol.clone()).collect())
        .collect()
}

fn has(rows: &[String], needle: &str) -> bool {
    rows.iter().any(|r| r.contains(needle))
}

fn browser_with(targets: Vec<LaunchTargetJson>) -> Browser {
    isolate_config();
    let mut browser = Browser::new(Vec::new());
    browser.show_launch_targets = true;
    browser.launch_targets = targets;
    browser
}

#[test]
fn play_as_uses_one_vocabulary() {
    let mut browser = browser_with(targets());
    browser.launch.saved = Some("windows-host".into());
    browser.launch_targets_index = 1;
    for (w, h) in SIZES {
        let rows = render(&browser, w, h);
        let screen = rows.join("\n");
        assert!(has(&rows, "Play as…"), "{screen}");
        assert!(has(&rows, "[your choice]"), "{screen}");
        assert!(has(&rows, "[auto]"), "{screen}");
        assert!(
            !has(&rows, "[saved]") && !has(&rows, "[default]"),
            "{screen}"
        );
        assert!(
            has(&rows, "Play uses: your choice, Windows · host Steam"),
            "{screen}"
        );
        assert!(has(&rows, "[d] use for Play"), "{screen}");
        assert!(has(&rows, "[i] install Steam runtime"), "{screen}");
        assert!(has(&rows, "[Esc] close"), "{screen}");
        assert!(has(&rows, "(needs Steam runtime)"), "{screen}");
    }
    assert!(browser.launch_target_needs_runtime());

    // Your choice gone: automatic.
    browser.launch_targets_index = 0;
    browser.launch.saved = Some("gone".into());
    let rows = render(&browser, 80, 24);
    assert!(!has(&rows, "[i] install"));
    assert!(has(&rows, "Play uses: automatic"));
}

#[test]
fn missing_build_offers_no_runtime_install() {
    let mut list = targets();
    list[1].missing = Some("build".into());
    let mut browser = browser_with(list);
    browser.launch_targets_index = 1;
    for (w, h) in SIZES {
        let rows = render(&browser, w, h);
        let screen = rows.join("\n");
        assert!(has(&rows, "(no build)"), "{screen}");
        assert!(has(&rows, "the Windows build is not installed"), "{screen}");
        assert!(!has(&rows, "[i]"), "{screen}");
    }
    browser.launch_target_install_runtime();
    assert!(!browser.launch.confirm_runtime);
    assert!(browser.launch_targets_status.is_empty());
}

#[test]
fn runtime_install_asks_first() {
    let mut browser = browser_with(targets());
    browser.launch_targets_index = 1;
    browser.launch_target_install_runtime();
    assert!(browser.launch.confirm_runtime);
    assert!(!browser.launch.runtime.is_running());
    for (w, h) in SIZES {
        let rows = render(&browser, w, h);
        let screen = rows.join("\n");
        assert!(has(&rows, "Install the Steam runtime"), "{screen}");
        assert!(has(&rows, "cancels."), "{screen}");
    }
    browser.launch_runtime_answer(false);
    assert!(!browser.launch.confirm_runtime);
    assert!(!browser.launch.runtime.is_running());
    assert_eq!(
        browser.launch_targets_status,
        "Steam runtime install cancelled."
    );
}

#[test]
fn long_target_list_scrolls_to_selection() {
    let mut list: Vec<LaunchTargetJson> = (0..11).map(|_| targets()[3].clone()).collect();
    let mut last = targets()[3].clone();
    last.steam_mode = Some("zz-last".into());
    list.push(last);
    let mut browser = browser_with(list);
    for _ in 0..11 {
        browser.launch_targets_next();
    }
    for (w, h) in SIZES {
        let rows = render(&browser, w, h);
        let screen = rows.join("\n");
        let row = rows.iter().find(|r| r.contains("windows · zz-last"));
        assert!(row.is_some_and(|r| r.contains('▶')), "{screen}");
        assert!(has(&rows, "[Esc] close"), "{screen}");
        assert!(has(&rows, "Play as…"), "{screen}");
    }
}

#[test]
fn play_as_shows_runtime_progress() {
    let browser = browser_with(targets());
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    assert!(browser.launch.runtime.start_with(move || {
        rx.recv().ok();
        Ok(())
    }));
    for (w, h) in SIZES {
        let rows = render(&browser, w, h);
        assert!(has(&rows, "Installing the Steam runtime"));
        assert!(has(&rows, "Play uses: automatic"));
    }
    tx.send(()).unwrap();
}

#[test]
fn unavailable_target_cannot_be_saved() {
    let mut browser = browser_with(targets());
    browser.launch_targets_index = 1;
    browser.launch_target_set_default();
    assert_eq!(
        browser.launch_targets_status,
        "Can't save an unavailable target."
    );
    assert_eq!(browser.launch.saved, None);
}

#[test]
fn status_line_replaces_and_clears_on_move() {
    let mut browser = browser_with(targets());
    browser.launch_targets_index = 2;
    browser.launch_target_set_default();
    assert!(
        browser
            .launch_targets_status
            .starts_with("Play now uses your choice")
    );
    browser.launch_target_set_automatic();
    assert_eq!(
        browser.launch_targets_status,
        "Play picks the target automatically."
    );
    for (w, h) in SIZES {
        let rows = render(&browser, w, h);
        let screen = rows.join("\n");
        assert!(!has(&rows, "Play now uses your choice"), "{screen}");
        assert!(
            has(&rows, "Play picks the target automatically."),
            "{screen}"
        );
    }
    browser.launch_targets_next();
    assert!(browser.launch_targets_status.is_empty());
    browser.launch_target_set_default();
    browser.launch_targets_previous();
    assert!(browser.launch_targets_status.is_empty());
    let rows = render(&browser, 80, 24);
    assert!(!has(&rows, "Play now uses"));
    browser.launch_target_set_automatic();
}
