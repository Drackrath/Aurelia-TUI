//! Expanded detail and Versions overlay layout.

use aurelia_tui::browse::{Browser, VersionsConfirm};
use aurelia_tui::interface::aurelia::{AvailableJson, DepotManifestInfo, LibraryGameJson};
use aurelia_tui::interface::game::Game;
use aurelia_tui::interface::library_info::{self as info, ExtendedInfoJson, RequirementsJson};
use aurelia_tui::ui;
use tui::layout::Rect;
use tui::{Terminal, backend::TestBackend};

fn isolate_config() {
    let dir = std::env::temp_dir().join(format!("aurelia-library-fix-{}", std::process::id()));
    // SAFETY: set before any config read.
    unsafe {
        std::env::set_var("AURELIA_TUI_DIR", &dir);
        std::env::set_var("AURELIA_TUI_CACHE_DIR", &dir);
        std::env::set_var("AURELIA_BIN", dir.join("missing-backend"));
    }
}

const SIZES: [(u16, u16); 2] = [(120, 40), (80, 24)];

fn game(id: u32, installed: bool) -> Game {
    let mut entry: LibraryGameJson = serde_json::from_value(serde_json::json!({
        "app_id": id,
        "name": "Baldur's Gate 3",
        "is_installed": installed,
        "install_path": if installed { Some("/games/bg3") } else { None },
        "playtime_forever_minutes": 8198,
        "size_on_disk": if installed { Some(154_770_000_000u64) } else { None },
        "local_manifest_ids": { "1086941": 4016443084035836922u64, "2330359": 77u64 },
    }))
    .unwrap();
    entry.store_url = Some(format!("https://store.steampowered.com/app/{id}"));
    Game::from_library(entry)
}

fn long_extended() -> ExtendedInfoJson {
    let features: Vec<String> = (0..30)
        .map(|i| format!("Feature number {}", i % 15))
        .collect();
    ExtendedInfoJson {
        genres: vec!["Adventure".into(), "RPG".into()],
        categories: features,
        metacritic: Some(96),
        website: Some("https://baldursgate3.game".into()),
        requirements: RequirementsJson {
            minimum: (0..30).map(|i| format!("Min line {i}")).collect(),
            recommended: vec!["Rec line".into()],
        },
        ..Default::default()
    }
}

fn render(w: u16, h: u16, f: impl FnOnce(&mut tui::Frame<TestBackend>)) -> String {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(f).unwrap();
    let buffer = terminal.backend().buffer().clone();
    let mut text = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            text.push_str(&buffer.get(x, y).symbol);
        }
        text.push('\n');
    }
    text
}

/// Detail pane like main.rs.
fn detail_text(g: &Game, expand: bool, scroll: u16, w: u16, h: u16) -> String {
    let pane_w = w * 6 / 10;
    let pane_h = h - 7;
    let value_w = (pane_w - 2) * 82 / 100 - 1;
    render(pane_w, pane_h, |f| {
        f.render_widget(
            ui::detail::detail_scrolled(Some(g), expand, value_w, pane_h - 2, scroll),
            f.size(),
        )
    })
}

#[test]
fn expanded_detail_keeps_core_rows_and_scrolls_to_requirements() {
    isolate_config();
    for (w, h) in SIZES {
        let id = 1_086_940 + u32::from(w);
        let g = game(id, true);
        info::store_extended(id as i32, None, Some(long_extended()));
        let collapsed = detail_text(&g, false, 0, w, h);
        assert!(collapsed.contains("[i] more"), "{w}x{h} hint: {collapsed}");
        let top = detail_text(&g, true, 0, w, h);
        for core in ["Playtime", "State", "Size", "Genres"] {
            assert!(top.contains(core), "{w}x{h} {core}: {top}");
        }
        assert!(top.contains("PgUp/PgDn"), "{w}x{h} title: {top}");
        assert!(top.contains("more · PgDn"), "{w}x{h} more marker: {top}");
        // Scroll past the end clamps.
        let bottom = detail_text(&g, true, u16::MAX, w, h);
        assert!(bottom.contains("Rec line"), "{w}x{h} bottom: {bottom}");
        assert!(bottom.contains("State"), "{w}x{h} core stays: {bottom}");
        assert_eq!(bottom.matches("Feature number 3,").count(), 0);
    }
}

#[test]
fn features_are_deduped() {
    let items: Vec<String> = ["Co-op", "Steam Cloud", "co-op", "Co-op"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    assert_eq!(ui::library_info::dedupe(&items), ["Co-op", "Steam Cloud"]);
}

#[test]
fn uninstalled_games_hide_size_and_path() {
    isolate_config();
    let g = game(42, false);
    let text = detail_text(&g, false, 0, 120, 40);
    assert!(text.contains("State"), "{text}");
    assert!(!text.contains("Size"), "{text}");
    assert!(!text.contains("Installation"), "{text}");
}

#[test]
fn list_shows_compact_size_at_46_columns() {
    isolate_config();
    let g = game(7, true);
    assert_eq!(
        ui::library_info::list_suffix(&g, 46).as_deref(),
        Some("155G")
    );
    assert_eq!(ui::library_info::compact_size(1_500_000_000), "1.5G");
    assert_eq!(ui::library_info::compact_size(512_000_000), "512M");
}

fn versions_browser() -> Browser {
    let app = 1_086_940;
    let mut browser = Browser::new(vec![game(app, true)]);
    browser.open_versions(app as i32);
    browser.versions_manifests = (0..30)
        .map(|i| DepotManifestInfo {
            depot_id: 1_086_941 + i / 3,
            depot_name: None,
            branch: ["public", "beta", "hotfix"][(i % 3) as usize].into(),
            manifest_id: if i == 1 {
                4_016_443_084_035_836_922
            } else {
                u64::from(i) + 1000
            },
            size: 1024,
        })
        .collect();
    browser.versions_available = Some(AvailableJson {
        app_id: app,
        available: true,
        install_path: Some("/games/bg3".into()),
        pinned: true,
        pinned_manifests: [("2378501".to_string(), 99u64)].into(),
    });
    browser
}

fn versions_text(browser: &Browser, w: u16, h: u16) -> String {
    let area = ui::centered_rect(72, 78, Rect::new(0, 0, w, h));
    render(w, h, |f| {
        f.render_widget(
            ui::versions::versions_sized(browser, area.width, area.height),
            area,
        )
    })
}

#[test]
fn versions_scrolls_and_keeps_footer() {
    isolate_config();
    for (w, h) in SIZES {
        let mut browser = versions_browser();
        let rows = browser.versions_rows();
        // Unlisted pinned and installed depots.
        assert_eq!(rows.len(), 32);
        assert!(
            rows.iter()
                .any(|r| r.depot_id == 2_378_501 && r.pinned == Some(99))
        );
        assert!(
            rows.iter()
                .any(|r| r.depot_id == 2_330_359 && r.installed == Some(77))
        );
        assert!(rows[1].is_installed());

        let top = versions_text(&browser, w, h);
        assert!(top.contains("[beta]"), "{w}x{h} branch: {top}");
        assert!(top.contains("· installed"), "{w}x{h}: {top}");
        assert!(top.contains("[Esc] close"), "{w}x{h} footer: {top}");
        assert!(top.contains("(1/32)"), "{w}x{h} title: {top}");

        browser.versions_previous();
        let end = versions_text(&browser, w, h);
        assert!(end.contains("(32/32)"), "{w}x{h}: {end}");
        assert!(end.contains("▶ 2378501"), "{w}x{h} selected visible: {end}");
        assert!(end.contains("pinned 99"), "{w}x{h}: {end}");
        assert!(end.contains("[Esc] close"), "{w}x{h} footer: {end}");
    }
}

#[test]
fn pin_and_downgrade_ask_first() {
    isolate_config();
    let mut browser = versions_browser();
    browser.request_pin();
    assert_eq!(browser.versions_confirm, Some(VersionsConfirm::Pin));
    let text = versions_text(&browser, 80, 24);
    assert!(text.contains("[y] confirm"), "{text}");
    browser.versions_confirm_key(false);
    assert_eq!(browser.versions_confirm, None);
    assert_eq!(browser.versions_status, "Cancelled.");

    browser.begin_downgrade_input();
    for c in "123".chars() {
        browser.versions_input_push(c);
    }
    browser.start_downgrade();
    assert_eq!(
        browser.versions_confirm,
        Some(VersionsConfirm::Downgrade {
            depot: 1_086_941,
            manifest: 123
        })
    );
    assert!(browser.downgrade_busy_text().is_none(), "nothing started");
    browser.versions_confirm_key(false);
    assert!(browser.versions_confirm.is_none());
}
