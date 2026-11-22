//! Renders "Play as…" launch extras.

use aurelia_tui::browse::Browser;
use aurelia_tui::interface::aurelia::LaunchTargetJson;
use aurelia_tui::interface::launch;
use aurelia_tui::ui;
use tui::Terminal;
use tui::backend::TestBackend;
use tui::layout::Rect;

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

fn render(browser: &Browser) -> String {
    let mut terminal = Terminal::new(TestBackend::new(110, 16)).unwrap();
    terminal
        .draw(|f| {
            f.render_widget(
                ui::launch_targets::launch_targets(browser),
                Rect::new(0, 0, 110, 16),
            )
        })
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol.clone())
        .collect()
}

#[test]
fn play_as_shows_saved_target_and_runtime_hint() {
    isolate_config();
    let mut browser = Browser::new(Vec::new());
    browser.show_launch_targets = true;
    browser.launch_targets = targets();
    browser.launch.saved = Some("windows-host".into());
    browser.launch_targets_index = 1;

    let screen = render(&browser);
    assert!(screen.contains("[saved]"), "{screen}");
    assert!(screen.contains("[default]"), "{screen}");
    assert!(
        screen.contains("Play uses: Windows · host Steam (saved)"),
        "{screen}"
    );
    assert!(screen.contains("[d] set as default"), "{screen}");
    assert!(screen.contains("[i] install Steam runtime"), "{screen}");
    assert!(browser.launch_target_needs_runtime());

    // Available target: no runtime hint.
    browser.launch_targets_index = 0;
    browser.launch.saved = Some("gone".into());
    let screen = render(&browser);
    assert!(!screen.contains("[i] install"), "{screen}");
    assert!(
        screen.contains("Play uses: default (gone unavailable)"),
        "{screen}"
    );
}

#[test]
fn play_as_shows_runtime_progress() {
    isolate_config();
    let mut browser = Browser::new(Vec::new());
    browser.show_launch_targets = true;
    browser.launch_targets = targets();
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    assert!(browser.launch.runtime.start_with(move || {
        rx.recv().ok();
        Ok(())
    }));
    let screen = render(&browser);
    assert!(screen.contains("Installing the Steam runtime"), "{screen}");
    assert!(screen.contains("Play uses: automatic"), "{screen}");
    tx.send(()).unwrap();
}

#[test]
fn unavailable_target_cannot_be_saved() {
    isolate_config();
    let mut browser = Browser::new(Vec::new());
    browser.launch_targets = targets();
    browser.launch_targets_index = 1;
    browser.launch_target_set_default();
    assert_eq!(
        browser.launch_targets_status,
        "Can't save an unavailable target."
    );
    assert_eq!(browser.launch.saved, None);
}
