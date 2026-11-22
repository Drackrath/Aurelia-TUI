//! Install picker options and language change.
use aurelia_tui::browse::Browser;
use aurelia_tui::browse::install_options::{InstallPickerOptions, LanguagePicker};
use aurelia_tui::interface::aurelia::LibraryGameJson;
use aurelia_tui::interface::game::Game;
use aurelia_tui::interface::install_options::{DryRunJson, PlatformChoice, parse_dry_run};
use aurelia_tui::ui;
use tui::Terminal;
use tui::backend::TestBackend;
use tui::layout::Rect;

fn isolate_config() {
    let dir = std::env::temp_dir().join("aurelia-tui-test-config");
    // SAFETY: set before any config read.
    unsafe {
        std::env::set_var("AURELIA_TUI_DIR", &dir);
        std::env::set_var("AURELIA_TUI_CACHE_DIR", &dir);
    }
}

fn browser(items: Vec<Game>) -> Browser {
    isolate_config();
    Browser::new(items)
}

fn dry(name: &str) -> DryRunJson {
    let path = format!(
        "{}/tests/fixtures/install_options/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    let value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    parse_dry_run(value).unwrap()
}

fn game(platform: Option<&str>) -> Game {
    Game::from_library(LibraryGameJson {
        app_id: 70,
        name: "Half-Life".into(),
        is_installed: true,
        install_path: Some("/g/hl".into()),
        update_available: false,
        is_owned: true,
        is_family_shared: false,
        platform: platform.map(str::to_string),
        active_branch: None,
        assets: None,
        store_url: None,
        library: Default::default(),
    })
}

fn render_text(draw: impl FnOnce(&mut tui::Frame<TestBackend>, Rect)) -> String {
    let mut term = Terminal::new(TestBackend::new(120, 20)).unwrap();
    term.draw(|f| {
        let area = f.size();
        draw(f, area)
    })
    .unwrap();
    let buf = term.backend().buffer().clone();
    buf.content.iter().map(|c| c.symbol.as_str()).collect()
}

#[test]
fn game_keeps_installed_platform() {
    assert_eq!(game(Some("linux")).platform.as_deref(), Some("linux"));
}

#[test]
fn picker_preselects_default_language() {
    let mut opts = InstallPickerOptions::default();
    opts.adopt(PlatformChoice::Auto, Some(dry("dry_run_multi.json")));
    assert_eq!(opts.language.as_deref(), Some("german"));
    let flags = opts.opts();
    assert_eq!(flags.platform, None);
    assert_eq!(flags.language.as_deref(), Some("german"));

    opts.cycle_language(true);
    assert_eq!(opts.language.as_deref(), Some("schinese"));
    opts.cycle_language(true);
    assert_eq!(opts.language.as_deref(), Some("english"));
    opts.cycle_language(false);
    assert_eq!(opts.language.as_deref(), Some("schinese"));
}

#[test]
fn single_language_adds_no_flag() {
    let mut opts = InstallPickerOptions {
        platform: PlatformChoice::Linux,
        ..Default::default()
    };
    opts.adopt(PlatformChoice::Linux, Some(dry("dry_run_single.json")));
    assert!(opts.languages().is_empty());
    let flags = opts.opts();
    assert_eq!(flags.platform.as_deref(), Some("linux"));
    assert_eq!(flags.language, None);
    // No linux depots, so blocked.
    assert!(opts.blocked());
}

#[test]
fn picker_renders_platform_and_language() {
    let mut b = browser(vec![]);
    b.show_install_picker = true;
    b.install_options
        .adopt(PlatformChoice::Auto, Some(dry("dry_run_multi.json")));
    let text = render_text(|f, area| f.render_widget(ui::install_picker::install_picker(&b), area));
    assert!(text.contains("auto (windows)"));
    assert!(text.contains("German"));
    assert!(text.contains("Download"));
}

#[test]
fn picker_hides_language_when_single() {
    let mut b = browser(vec![]);
    b.install_options
        .adopt(PlatformChoice::Auto, Some(dry("dry_run_single.json")));
    let text = render_text(|f, area| f.render_widget(ui::install_picker::install_picker(&b), area));
    assert!(text.contains("Platform"));
    assert!(!text.contains("Language"));
}

#[test]
fn language_picker_preselects_installed() {
    let picker = LanguagePicker::from_dry_run(70, "HL".into(), &dry("dry_run_installed.json"))
        .expect("three languages");
    assert_eq!(picker.selected().map(String::as_str), Some("german"));
    assert!(picker.is_current("german"));
    assert!(LanguagePicker::from_dry_run(70, "HL".into(), &dry("dry_run_single.json")).is_none());
}

#[test]
fn single_language_game_gets_notice() {
    let g = game(Some("linux"));
    let mut b = browser(vec![g.clone()]);
    b.adopt_language_dry_run(&g, &dry("dry_run_single.json"));
    assert!(b.language_picker.is_none());
    assert!(
        b.notice
            .as_deref()
            .unwrap()
            .contains("only available in one language")
    );
}

#[test]
fn current_language_is_not_a_change() {
    let g = game(Some("linux"));
    let mut b = browser(vec![g.clone()]);
    b.adopt_language_dry_run(&g, &dry("dry_run_installed.json"));
    assert!(!b.confirm_language_change());
    assert!(
        b.notice
            .as_deref()
            .unwrap()
            .contains("Already installed in German")
    );
    assert!(b.language_picker.is_some());
}

#[test]
fn language_picker_renders_labels() {
    let g = game(Some("linux"));
    let mut b = browser(vec![g.clone()]);
    b.adopt_language_dry_run(&g, &dry("dry_run_installed.json"));
    let text =
        render_text(|f, area| f.render_widget(ui::install_options::language_picker(&b), area));
    assert!(text.contains("French"));
    assert!(text.contains("installed"));
}

#[test]
fn uninstall_prompt_offers_prefix_choice() {
    let text = render_text(|f, area| {
        f.render_widget(ui::install_options::confirm_uninstall_game("HL"), area)
    });
    assert!(text.contains("keep Wine prefix"));
    assert!(text.contains("delete prefix"));
}
