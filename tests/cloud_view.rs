//! Cloud overlay scrolling, confirms and badges.

use aurelia_tui::browse::Browser;
use aurelia_tui::interface::aurelia::{CloudDirection, CloudFileJson, LibraryGameJson};
use aurelia_tui::interface::cloud_status;
use aurelia_tui::interface::cloud_sync::parse_sync;
use aurelia_tui::interface::game::Game;
use aurelia_tui::ui;
use crossterm::event::KeyCode;
use tui::Terminal;
use tui::backend::TestBackend;
use tui::layout::Rect;

/// Private config and cache dirs.
fn isolate() {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("cloud-view-test");
    std::fs::create_dir_all(&dir).unwrap();
    // SAFETY: set before any reads.
    unsafe {
        std::env::set_var("AURELIA_TUI_DIR", &dir);
        std::env::set_var("AURELIA_TUI_CACHE_DIR", &dir);
    }
}

fn game(id: u32, name: &str) -> Game {
    Game::from_library(LibraryGameJson {
        app_id: id,
        name: name.to_string(),
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

fn with_files(n: usize) -> Browser {
    let mut b = Browser::new(vec![game(1086940, "Baldur's Gate 3")]);
    b.show_cloud = true;
    b.cloud_files = (0..n)
        .map(|i| CloudFileJson {
            filename: format!("_SAVE_Public/Savegames/Story/save_{i:03}.lsv"),
            size: 1000 + i as u64,
            timestamp: 0,
        })
        .collect();
    b
}

fn text(t: &Terminal<TestBackend>) -> Vec<String> {
    let buf = t.backend().buffer();
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf.get(x, y).symbol.clone())
                .collect()
        })
        .collect()
}

fn draw_overlay(b: &Browser, w: u16, h: u16) -> Vec<String> {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| {
        let area = ui::centered_rect(70, 80, f.size());
        ui::cloud::draw(f, b, area);
    })
    .unwrap();
    text(&t)
}

#[test]
fn overlay_scrolls_and_keeps_hints_at_both_sizes() {
    isolate();
    for (w, h) in [(120, 40), (80, 24)] {
        let mut b = with_files(60);
        let screen = draw_overlay(&b, w, h);
        assert!(screen.iter().any(|l| l.contains("save_000")));
        assert!(
            screen.iter().any(|l| l.contains(ui::cloud::HINTS)),
            "{w}x{h}"
        );

        b.cloud_key(KeyCode::Char('G'));
        assert_eq!(b.cloud_view.index, 59);
        let screen = draw_overlay(&b, w, h);
        assert!(screen.iter().any(|l| l.contains("save_059")), "{w}x{h}");
        assert!(!screen.iter().any(|l| l.contains("save_000")));
        assert!(screen.iter().any(|l| l.contains("Steam Cloud · 60/60")));
        assert!(screen.iter().any(|l| l.contains(ui::cloud::HINTS)));

        b.cloud_key(KeyCode::PageUp);
        b.cloud_key(KeyCode::Char('k'));
        assert_eq!(b.cloud_view.index, 48);
        let screen = draw_overlay(&b, w, h);
        assert!(screen.iter().any(|l| l.contains("save_048")));

        b.cloud_key(KeyCode::Char('g'));
        b.cloud_key(KeyCode::Char('j'));
        b.cloud_key(KeyCode::PageDown);
        assert_eq!(b.cloud_view.index, 11);
    }
}

#[test]
fn up_and_down_ask_first_and_cancel() {
    isolate();
    for (w, h) in [(120, 40), (80, 24)] {
        let mut b = with_files(3);
        b.cloud_key(KeyCode::Char('u'));
        assert_eq!(b.cloud_view.confirm, Some(CloudDirection::Up));
        let screen = draw_overlay(&b, w, h).join("\n");
        assert!(
            screen.contains("Upload local saves to Steam Cloud?"),
            "{w}x{h}"
        );
        assert!(screen.contains("[y]"));
        // Any other key cancels.
        b.cloud_key(KeyCode::Char('n'));
        assert_eq!(b.cloud_view.confirm, None);
        assert_eq!(b.cloud_status, "Cancelled.");
        assert!(b.show_cloud);

        b.cloud_key(KeyCode::Char('d'));
        let screen = draw_overlay(&b, w, h).join("\n");
        assert!(screen.contains("Download Steam Cloud saves to this PC?"));
        b.cloud_key(KeyCode::Esc);
        assert_eq!(b.cloud_view.confirm, None);
        assert!(b.show_cloud, "Esc only cancels the confirm");
    }
}

fn draw_library(b: &Browser, w: u16, h: u16) -> Vec<String> {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| {
        let half = f.size().width / 2;
        f.render_widget(
            ui::list::list(b, half, 0),
            Rect::new(0, 0, half, f.size().height),
        );
        let sel = b.selected();
        f.render_widget(
            ui::detail::detail(sel.as_ref(), false, half - 10, f.size().height - 2),
            Rect::new(half, 0, f.size().width - half, f.size().height),
        );
    })
    .unwrap();
    text(&t)
}

#[test]
fn badge_hidden_until_a_sync_ran() {
    isolate();
    for (w, h) in [(120, 40), (80, 24)] {
        // Never synced: no marker, no row.
        let fresh = Browser::new(vec![game(424242, "Never Synced")]);
        let screen = draw_library(&fresh, w, h).join("\n");
        assert!(screen.contains("Never Synced"));
        assert!(!screen.contains('☁'), "{w}x{h}");
        assert!(!screen.contains("Cloud "));
        assert!(!screen.contains("error"));
    }

    let ok = parse_sync(serde_json::json!({"status": "ok", "downloaded": ["a"]})).unwrap();
    cloud_status::record_result(515151, &ok);
    for (w, h) in [(120, 40), (80, 24)] {
        let synced = Browser::new(vec![game(515151, "Was Synced")]);
        let screen = draw_library(&synced, w, h).join("\n");
        assert!(screen.contains("Was Synced ☁"), "{w}x{h}");
        assert!(screen.contains("synced just now"));
    }
}
