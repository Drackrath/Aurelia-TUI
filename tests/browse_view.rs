//! Exercises the browse model (tab filters + fuzzy query + sort) and renders the
//! browse widgets into tui's off-screen `TestBackend` to prove they lay out and
//! paint the expected content.

use aurelia_tui::browse::{Browser, Filter};
use aurelia_tui::interface::aurelia::LibraryGameJson;
use aurelia_tui::interface::game::Game;
use aurelia_tui::{theme, ui};
use tui::backend::TestBackend;
use tui::layout::{Constraint, Direction, Layout, Rect};
use tui::Terminal;

/// Point the config at a throwaway dir so tests don't read/write the real one.
fn isolate_config() {
    let dir = std::env::temp_dir().join("aurelia-tui-test-config");
    // SAFETY: set before any Config::cached() call in this test process.
    unsafe {
        std::env::set_var("AURELIA_TUI_DIR", &dir);
        std::env::set_var("AURELIA_TUI_CACHE_DIR", &dir);
    }
}

fn game(id: u32, name: &str, installed: bool, update: bool) -> Game {
    Game::from_library(LibraryGameJson {
        app_id: id,
        name: name.to_string(),
        is_installed: installed,
        install_path: None,
        update_available: update,
        is_owned: true,
        is_family_shared: false,
        platform: None,
        active_branch: None,
        assets: None,
        store_url: None,
    })
}

#[test]
fn filter_and_query_narrow_the_view() {
    isolate_config();
    let games = vec![
        game(1, "Alpha", true, false),
        game(2, "Beta", false, false),
        game(3, "Gamma", true, true),
    ];
    let mut browser = Browser::new(games);
    assert_eq!(browser.visible().len(), 3, "All shows everything");

    browser.set_filter(Filter::Installed);
    assert_eq!(browser.visible().len(), 2, "Installed: Alpha + Gamma");

    browser.set_filter(Filter::Updates);
    assert_eq!(browser.visible().len(), 1, "Updates: only Gamma");

    browser.set_filter(Filter::All);
    browser.push_query('l'); // fuzzy-matches "Alpha"
    let names: Vec<String> = browser
        .visible()
        .iter()
        .map(|g| g.raw_name().to_string())
        .collect();
    assert!(names.contains(&"Alpha".to_string()));
    assert!(!names.contains(&"Beta".to_string()));
}

#[test]
fn counts_reflect_library() {
    isolate_config();
    let browser = Browser::new(vec![
        game(1, "Alpha", true, false),
        game(2, "Beta", false, false),
        game(3, "Gamma", true, true),
    ]);
    let c = browser.counts();
    assert_eq!(c.total, 3);
    assert_eq!(c.installed, 2);
    assert_eq!(c.updates, 1);
}

#[test]
fn browse_widgets_render_without_panicking() {
    isolate_config();
    let mut browser = Browser::new(vec![
        game(1, "Alpha", true, false),
        game(2, "Beta", false, true),
    ]);
    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            f.render_widget(ui::tabs::tabs(&browser), Rect::new(0, 0, 80, 3));
            let list = ui::list::list(&browser);
            f.render_stateful_widget(list, Rect::new(0, 3, 40, 18), &mut browser.state);
            let selected = browser.selected();
            f.render_widget(
                ui::detail::detail(selected.as_ref(), false, 30, 16),
                Rect::new(40, 3, 40, 18),
            );
            f.render_widget(
                ui::status::status_bar(&browser, Some("tester")),
                Rect::new(0, 22, 80, 2),
            );
        })
        .unwrap();

    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol.clone())
        .collect();

    assert!(text.contains("Alpha"), "list shows a game name");
    assert!(text.contains("tester"), "status bar shows the account");
    assert!(text.contains("installed"), "status bar shows counts");
    assert!(text.contains("Detail"), "detail panel renders");
}

/// Render the complete browse frame the way `main` lays it out — tabs / [list |
/// cover+detail] / status — with a game that has no description (so the right
/// pane has only two chunks). Regression for an index-out-of-bounds panic where
/// the status bar read the wrong layout.
#[test]
fn full_browse_frame_renders() {
    isolate_config();
    let mut browser = Browser::new(vec![
        game(1, "Alpha", true, false),
        game(2, "Beta", false, true),
    ]);
    let backend = TestBackend::new(80, 30);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(3),
                    Constraint::Min(2),
                    Constraint::Length(2),
                ])
                .split(f.size());
            f.render_widget(ui::tabs::tabs(&browser), chunks[0]);

            let body = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
                .split(chunks[1]);
            let list = ui::list::list(&browser);
            f.render_stateful_widget(list, body[0], &mut browser.state);

            let selected = browser.selected();
            // No description in tests → right pane is cover + table (two chunks).
            let right_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints(vec![Constraint::Min(0), Constraint::Length(13)])
                .split(body[1]);
            f.render_widget(theme::panel("Cover".to_string()), right_chunks[0]);
            f.render_widget(
                ui::detail::detail(selected.as_ref(), false, 38, 11),
                right_chunks[1],
            );

            // Status bar at the OUTER bottom chunk — the regression point.
            f.render_widget(
                ui::status::status_bar(&browser, Some("tester")),
                chunks[2],
            );
        })
        .unwrap();

    let buf = terminal.backend().buffer();
    let bottom: String = (28..30)
        .flat_map(|y| (0..80u16).map(move |x| (x, y)))
        .map(|(x, y)| buf.get(x, y).symbol.clone())
        .collect();
    assert!(bottom.contains("tester"), "status bar renders at the bottom");
}

/// The cover panel and the detail panel must occupy separate, ordered rows so
/// the artwork never overlaps the details (regression for the overlap fix).
#[test]
fn cover_and_detail_do_not_overlap() {
    isolate_config();
    let browser = Browser::new(vec![game(1, "Alpha", true, false)]);
    let backend = TestBackend::new(60, 24);
    let mut terminal = Terminal::new(backend).unwrap();

    // Mirror main.rs's right-pane split: Cover (top 40%) over Detail (bottom 60%).
    let right = Rect::new(0, 0, 60, 24);
    terminal
        .draw(|f| {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
                .split(right);
            f.render_widget(theme::panel("Cover".to_string()), chunks[0]);
            f.render_widget(
                ui::detail::detail(browser.selected().as_ref(), false, 40, 12),
                chunks[1],
            );
        })
        .unwrap();

    // Find which row each panel title lands on.
    let buf = terminal.backend().buffer();
    let row_text = |y: u16| -> String {
        (0..60).map(|x| buf.get(x, y).symbol.clone()).collect()
    };
    let cover_row = (0..24).find(|&y| row_text(y).contains("Cover"));
    let detail_row = (0..24).find(|&y| row_text(y).contains("Detail"));

    let cover_row = cover_row.expect("Cover title rendered");
    let detail_row = detail_row.expect("Detail title rendered");
    assert!(
        cover_row < detail_row,
        "Cover panel must sit above Detail (cover row {cover_row}, detail row {detail_row})"
    );
    // The detail panel must start at or below the cover panel's bottom border.
    assert!(
        detail_row as i32 - cover_row as i32 >= 8,
        "Cover and Detail occupy clearly separate bands"
    );
}
