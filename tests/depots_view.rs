//! Depots and depot files overlay renders.

use aurelia_tui::browse::Browser;
use aurelia_tui::interface::aurelia::{DepotJson, LibraryGameJson};
use aurelia_tui::interface::depot_files::{DepotFileJson, DepotFilesJson};
use aurelia_tui::interface::game::Game;
use aurelia_tui::restart_depots::{
    self as rd, DownloadState, FileBrowser, FileInput, RESTART_QUESTION, files_key,
};
use aurelia_tui::ui;
use aurelia_tui::ui::restart_depots as view;
use crossterm::event::KeyCode;
use tui::Terminal;
use tui::backend::TestBackend;

const SIZES: [(u16, u16); 2] = [(120, 40), (80, 24)];

fn isolate_config() {
    let dir = std::env::temp_dir().join("aurelia-tui-test-depots");
    // SAFETY: set before config reads.
    unsafe {
        std::env::set_var("AURELIA_TUI_DIR", &dir);
        std::env::set_var("AURELIA_TUI_CACHE_DIR", &dir);
    }
}

fn game() -> Game {
    Game::from_library(LibraryGameJson {
        app_id: 1086940,
        name: "Baldur's Gate 3".into(),
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

fn depot(id: u64, name: &str, size: u64, config: &str) -> DepotJson {
    DepotJson {
        id,
        name: name.into(),
        size,
        config: config.into(),
        ..DepotJson::default()
    }
}

fn screen(term: &Terminal<TestBackend>) -> Vec<String> {
    let buf = term.backend().buffer();
    let area = buf.area;
    (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buf.get(x, y).symbol.clone())
                .collect()
        })
        .collect()
}

#[test]
fn depot_name_hides_placeholder() {
    assert_eq!(rd::depot_name(228989, "Depot 228989"), None);
    assert_eq!(rd::depot_name(228989, ""), None);
    assert_eq!(
        rd::depot_name(1086941, "BG3 Linux"),
        Some("BG3 Linux".into())
    );
}

#[test]
fn list_jump_keys() {
    assert_eq!(rd::list_jump(5, 58, KeyCode::Char('G')), 57);
    assert_eq!(rd::list_jump(5, 58, KeyCode::Char('g')), 0);
    assert_eq!(rd::list_jump(5, 58, KeyCode::PageDown), 15);
    assert_eq!(rd::list_jump(55, 58, KeyCode::PageDown), 57);
    assert_eq!(rd::list_jump(5, 58, KeyCode::PageUp), 0);
    assert_eq!(rd::list_jump(0, 0, KeyCode::Char('G')), 0);
}

#[test]
fn depots_overlay_rows_and_scroll() {
    isolate_config();
    let mut browser = Browser::new(vec![game()]);
    browser.depots = (0..58)
        .map(|i| depot(228989 + i, &format!("Depot {}", 228989 + i), 0, ""))
        .collect();
    browser.depots[1] = depot(
        228990,
        "BG3 Windows",
        150 * 1024 * 1024 * 1024,
        "os: windows, lang: german",
    );
    browser.show_depots = true;
    rd::depots_key(&mut browser, KeyCode::Char('G'));
    assert_eq!(browser.depots_scroll, 57);

    for (w, h) in SIZES {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| {
            let area = view::overlay_area(f.size());
            assert!(area.y >= 3, "below the tab bar");
            assert!(area.y + area.height <= h - 2, "above the status bar");
            let rows = area.height.saturating_sub(2) as usize;
            f.render_widget(ui::depots::depots(&browser, rows), area);
        })
        .unwrap();
        let rows = screen(&term);
        let text = rows.join("\n");
        assert!(text.contains(&ui::depots::title(58)), "{w}x{h} title");
        let last = rows.iter().find(|r| r.contains("229046"));
        assert!(
            last.is_some_and(|r| r.contains('▶')),
            "{w}x{h} last selected"
        );
        assert!(!text.contains("(229046)"), "no duplicated id");
        assert!(!text.contains("0 B"), "unknown size hidden");
    }

    // Known name, size and config show.
    browser.depots_scroll = 1;
    let mut term = Terminal::new(TestBackend::new(120, 40)).unwrap();
    term.draw(|f| {
        let area = view::overlay_area(f.size());
        f.render_widget(ui::depots::depots(&browser, 30), area);
    })
    .unwrap();
    let text = screen(&term).join("\n");
    assert!(text.contains("228990  BG3 Windows  161.06 GB"), "{text}");
    assert!(text.contains("[os: windows, lang: german]"));
}

fn many_files() -> DepotFilesJson {
    DepotFilesJson {
        app_id: 1086940,
        depot_id: 2330356,
        manifest_id: 8907182068373780168,
        total_size: 40 * 585,
        files: (0..40)
            .map(|i| DepotFileJson {
                path: format!("Data/Localization/file{i:02}.lsx"),
                kind: "file".into(),
                size: 585,
                sha1: "de9f2c7fd25e1b3afad3e85a0bd17d9b100db4b3".into(),
                chunks: 1,
                ..DepotFileJson::default()
            })
            .collect(),
    }
}

fn file_browser() -> FileBrowser {
    let mut files = FileBrowser {
        app_id: 1086940,
        depot_id: 2330356,
        depot_label: "Depot 2330356".into(),
        last_dir: "~/Downloads".into(),
        ..FileBrowser::default()
    };
    files.apply_listing(Ok(many_files()));
    files
}

#[test]
fn files_overlay_scrolls_and_keeps_hints() {
    let mut files = file_browser();
    files_key(&mut files, KeyCode::Char('G'));
    files_key(&mut files, KeyCode::Enter);
    assert_eq!(files.index, 39);

    for (w, h) in SIZES {
        let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
        term.draw(|f| {
            let area = view::overlay_area(f.size());
            view::draw_files(f, &files, area);
        })
        .unwrap();
        let rows = screen(&term);
        let text = rows.join("\n");
        assert!(
            text.contains("Depot 2330356 · manifest 8907182068373780168"),
            "{w}x{h} title"
        );
        assert!(!text.contains("Depot Depot"));
        assert!(text.contains("file39.lsx"), "{w}x{h} selected visible");
        assert!(text.contains(view::FILE_HINTS), "{w}x{h} hints whole");
        assert!(text.contains("SHA-1 de9f"), "{w}x{h} details");
        assert!(
            rows.iter().take(3).all(|r| !r.contains("Files")),
            "tab bar uncovered"
        );
    }
}

#[test]
fn download_needs_folder_enter_and_confirm() {
    let mut files = file_browser();
    files_key(&mut files, KeyCode::Char('d'));
    assert_eq!(files.input.as_ref().unwrap().0, FileInput::Target);
    // Folder prompt waits for Enter.
    files_key(&mut files, KeyCode::Char('x'));
    assert!(files.confirm.is_none());
    files_key(&mut files, KeyCode::Backspace);
    files_key(&mut files, KeyCode::Enter);
    let request = files.confirm.clone().expect("asks first");
    assert_eq!(request.out_dir, "~/Downloads");
    assert_eq!(request.path, "Data/Localization/file00.lsx");

    // Anything but y cancels.
    files_key(&mut files, KeyCode::Enter);
    assert!(files.confirm.is_none());
    assert_eq!(files.download_state(), DownloadState::Idle);
}

#[test]
fn questions_fit_both_sizes() {
    let long = "Download Baldur's Gate 3.app/Contents/Data/Localization/language.lsx (585 B) to ~/Downloads? (y/n)";
    for (w, h) in SIZES {
        for (question, detail) in [
            (RESTART_QUESTION, ""),
            (long, ""),
            (
                "Push anyway? (y/n)",
                "refusing to push: local collections were never pulled from Steam. Run `aurelia collections pull` first, or pass `--force` to upload anyway.",
            ),
        ] {
            let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
            term.draw(|f| {
                let area = view::question_area(f.size(), question, detail);
                assert!(area.width <= w && area.height <= h);
                f.render_widget(
                    view::question_box(question.to_string(), detail.to_string()),
                    area,
                );
            })
            .unwrap();
            let rows = screen(&term);
            let joined: String = rows
                .iter()
                .map(|r| r.trim_matches(|c: char| c == '│' || c == ' ').to_string())
                .filter(|r| !r.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            let words_ok = question.split(' ').all(|word| joined.contains(word));
            assert!(words_ok, "{w}x{h}: {question}\n{}", rows.join("\n"));
            assert!(joined.contains("[y] confirm"), "{w}x{h} keys");
        }
    }
}
