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

fn screen(w: u16, h: u16, draw: impl FnOnce(&mut tui::Frame<TestBackend>)) -> Vec<String> {
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(draw).unwrap();
    let buf = term.backend().buffer().clone();
    (0..h)
        .map(|y| (0..w).map(|x| buf.get(x, y).symbol.clone()).collect())
        .collect()
}

const SIZES: [(u16, u16); 2] = [(120, 40), (80, 24)];

fn many_languages() -> DryRunJson {
    let languages: Vec<String> = (0..29).map(|i| format!("lang{i:02}")).collect();
    DryRunJson {
        app_id: 70,
        platform: "windows".into(),
        depot_count: 1,
        default_language: Some("lang00".into()),
        installed_language: Some("lang00".into()),
        languages,
        ..Default::default()
    }
}

fn libraries(n: usize) -> Vec<aurelia_tui::interface::aurelia::LibraryJson> {
    (0..n)
        .map(|i| aurelia_tui::interface::aurelia::LibraryJson {
            path: format!("/mnt/lib{i:02}"),
            free_bytes: Some(500_000_000_000),
        })
        .collect()
}

#[test]
fn language_picker_renders_labels() {
    let g = game(Some("linux"));
    let mut b = browser(vec![g.clone()]);
    b.adopt_language_dry_run(&g, &dry("dry_run_installed.json"));
    for (w, h) in SIZES {
        let text = screen(w, h, |f| ui::install_options::draw_language_picker(f, &b)).join("\n");
        assert!(text.contains("French"));
        assert!(text.contains("installed"));
        assert!(text.contains("[Esc] cancel"), "{w}x{h} footer");
    }
}

#[test]
fn language_picker_scrolls_to_last() {
    let g = game(Some("windows"));
    let mut b = browser(vec![g.clone()]);
    b.adopt_language_dry_run(&g, &many_languages());
    for _ in 0..40 {
        b.language_picker.as_mut().unwrap().next();
    }
    for (w, h) in SIZES {
        let rows = screen(w, h, |f| ui::install_options::draw_language_picker(f, &b));
        let marked = rows
            .iter()
            .find(|r| r.contains('▶'))
            .expect("selection visible");
        assert!(marked.contains("Lang28"), "{w}x{h}");
        assert!(rows.join("\n").contains("[Enter] switch"));
    }
}

#[test]
fn unknown_current_language_is_said() {
    let g = game(Some("windows"));
    let mut b = browser(vec![g.clone()]);
    b.adopt_language_dry_run(&g, &dry("dry_run_multi.json"));
    let text = screen(80, 24, |f| ui::install_options::draw_language_picker(f, &b)).join("\n");
    assert!(text.contains("Current language unknown"));
    assert!(!text.contains("● installed"));
}

#[test]
fn language_picker_loads_in_background() {
    use aurelia_tui::browse::install_options::OptionsMsg;
    let g = game(Some("windows"));
    let mut b = browser(vec![g.clone()]);
    b.language_picker = Some(LanguagePicker {
        app_id: 70,
        game_name: "HL".into(),
        loading: true,
        ..Default::default()
    });
    let text = screen(80, 24, |f| ui::install_options::draw_language_picker(f, &b)).join("\n");
    assert!(text.contains("Loading languages"));
    assert!(!b.confirm_language_change());
    b.apply_options_msg(OptionsMsg::Languages {
        app_id: 70,
        result: Ok(dry("dry_run_single.json")),
    });
    assert!(b.language_picker.is_none());
    assert!(b.notice.unwrap().contains("only available in one language"));
}

#[test]
fn picker_shows_sizing_then_result() {
    use aurelia_tui::browse::install_options::OptionsMsg;
    let mut b = browser(vec![]);
    b.install_libraries = libraries(2);
    b.install_options.pending.insert(PlatformChoice::Auto);
    let text = screen(80, 24, |f| ui::install_picker::draw(f, &b)).join("\n");
    assert!(text.contains("Sizing…"));
    b.apply_options_msg(OptionsMsg::Sized {
        app_id: 0,
        platform: PlatformChoice::Auto,
        dry: Some(dry("dry_run_multi.json")),
    });
    assert_eq!(b.install_estimate, Some(9_663_676_416));
    let text = screen(80, 24, |f| ui::install_picker::draw(f, &b)).join("\n");
    assert!(!text.contains("Sizing…"));
    assert!(text.contains("German"));
}

#[test]
fn zero_download_reads_unknown() {
    let mut b = browser(vec![]);
    b.install_libraries = libraries(1);
    let mut d = dry("dry_run_multi.json");
    d.download_size = 0;
    b.install_options.adopt(PlatformChoice::Auto, Some(d));
    let text = screen(120, 40, |f| ui::install_picker::draw(f, &b)).join("\n");
    assert!(text.contains("Download  unknown"));
}

#[test]
fn store_platforms_limit_choices() {
    let mut opts = InstallPickerOptions {
        platform: PlatformChoice::Linux,
        ..Default::default()
    };
    assert_eq!(opts.choices().len(), 3, "unknown offers all");
    opts.set_store_platforms(vec!["Windows".into()]);
    assert_eq!(opts.platform, PlatformChoice::Auto, "linux dropped");
    assert_eq!(
        opts.choices(),
        [PlatformChoice::Auto, PlatformChoice::Windows]
    );
    assert_eq!(opts.next_platform(), PlatformChoice::Windows);
    opts.platform = PlatformChoice::Windows;
    assert_eq!(opts.next_platform(), PlatformChoice::Auto);
    opts.set_store_platforms(vec![]);
    assert_eq!(opts.choices().len(), 3, "empty means unknown");
}

#[test]
fn picker_hints_fit_and_selection_scrolls() {
    let mut b = browser(vec![]);
    b.install_libraries = libraries(30);
    b.install_options
        .adopt(PlatformChoice::Auto, Some(dry("dry_run_multi.json")));
    for _ in 0..40 {
        b.install_picker_next();
    }
    for (w, h) in SIZES {
        let rows = screen(w, h, |f| ui::install_picker::draw(f, &b));
        let text = rows.join("\n");
        assert!(text.contains("which library?"), "{w}x{h} title");
        for hint in [
            "[p] platform",
            "[←/→ or h/l] language",
            "[Enter] install",
            "[Esc] cancel",
        ] {
            assert!(text.contains(hint), "{w}x{h} missing {hint}");
        }
        let marked = rows
            .iter()
            .find(|r| r.contains('▶'))
            .expect("selection visible");
        assert!(marked.contains("/mnt/lib29"), "{w}x{h}");
    }
}

#[test]
fn uninstall_prefix_needs_explicit_toggle() {
    let g = game(Some("windows"));
    let mut b = browser(vec![g.clone()]);
    b.confirm_uninstall = true;
    assert!(!b.uninstall_deletes_prefix(), "keeps prefix by default");
    for (w, h) in SIZES {
        let text = screen(w, h, |f| ui::install_options::draw_uninstall(f, &b, &g)).join("\n");
        assert!(text.contains("[ ] also delete Wine prefix"), "{w}x{h}");
        assert!(text.contains("saves inside it are lost"));
        assert!(text.contains("[p] toggle prefix") && text.contains("[y] uninstall"));
        assert!(text.contains("cancel"), "{w}x{h} footer");
    }
    b.toggle_uninstall_prefix();
    assert!(b.uninstall_deletes_prefix());
    let text = screen(80, 24, |f| ui::install_options::draw_uninstall(f, &b, &g)).join("\n");
    assert!(text.contains("[x] also delete Wine prefix"));
    b.close_uninstall_prompt();
    assert!(!b.confirm_uninstall && !b.uninstall_delete_prefix);
}

#[test]
fn native_linux_uninstall_has_no_prefix() {
    let g = game(Some("linux"));
    let mut b = browser(vec![g.clone()]);
    b.toggle_uninstall_prefix();
    assert!(!b.uninstall_deletes_prefix());
    let text = screen(80, 24, |f| ui::install_options::draw_uninstall(f, &b, &g)).join("\n");
    assert!(!text.contains("prefix"));
    assert!(text.contains("Uninstall Half-Life?"));
}
