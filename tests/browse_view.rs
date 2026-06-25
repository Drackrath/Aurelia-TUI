//! Exercises the browse model (tab filters + fuzzy query + sort) and renders the
//! browse widgets into tui's off-screen `TestBackend` to prove they lay out and
//! paint the expected content.

use aurelia_tui::browse::{Browser, Filter};
use aurelia_tui::interface::aurelia::{
    LibraryGameJson, ProtonJson, WorkshopCommentJson, WorkshopItemJson,
};
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

/// The Steam Cloud overlay advertises the sync directions in its title so the
/// directional keys (`d` down-only, `u` up-only) are discoverable next to the
/// plain `s` sync.
#[test]
fn cloud_overlay_shows_directional_hints() {
    isolate_config();
    let browser = Browser::new(vec![game(1, "Alpha", true, false)]);
    let backend = TestBackend::new(80, 12);
    let mut terminal = Terminal::new(backend).unwrap();

    terminal
        .draw(|f| {
            f.render_widget(ui::cloud::cloud(&browser), Rect::new(0, 0, 80, 12));
        })
        .unwrap();

    let title: String = (0..80)
        .map(|x| terminal.backend().buffer().get(x, 0).symbol.clone())
        .collect();
    assert!(title.contains("[s] sync"), "title keeps the both-ways sync hint");
    assert!(title.contains("[d] down"), "title advertises download-only");
    assert!(title.contains("[u] up"), "title advertises upload-only");
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

fn proton(name: &str, label: &str, installed: bool) -> ProtonJson {
    ProtonJson {
        name: name.to_string(),
        label: label.to_string(),
        size: 0,
        installed,
        is_default: false,
    }
}

/// Render the whole proton overlay into a wide buffer and return every cell as
/// one string, so we can assert on the title hints regardless of row.
fn render_proton(browser: &Browser) -> String {
    let backend = TestBackend::new(90, 16);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| f.render_widget(ui::proton::proton(browser), f.size()))
        .unwrap();
    let buf = terminal.backend().buffer();
    (0..16)
        .map(|y| (0..90).map(|x| buf.get(x, y).symbol.clone()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Only an installed custom (GE) runtime is uninstallable; the overlay surfaces
/// the [u] uninstall hint exactly then, and always offers [i] install.
#[test]
fn proton_overlay_offers_install_and_guards_uninstall() {
    isolate_config();
    let mut browser = Browser::new(vec![game(1, "Alpha", true, false)]);
    browser.protons = vec![
        proton("Proton 9.0", "Valve", true),         // installed but not removable
        proton("GE-Proton11-1", "Proton-GE", false), // available, not installed
        proton("GE-Proton10-34", "Proton-GE", true), // installed custom -> removable
    ];

    // Highlight the Valve runtime: install is always offered, uninstall is not.
    browser.proton_index = 0;
    assert!(!browser.selected_proton_uninstallable());
    let valve = render_proton(&browser);
    assert!(valve.contains("[i] install"), "install hint always present");
    assert!(
        !valve.contains("[u] uninstall"),
        "Valve runtime must not offer uninstall"
    );

    // Highlight the installed GE runtime: uninstall is offered.
    browser.proton_index = 2;
    assert!(browser.selected_proton_uninstallable());
    assert!(
        render_proton(&browser).contains("[u] uninstall"),
        "installed custom runtime offers uninstall"
    );

    // A not-installed runtime is never uninstallable.
    browser.proton_index = 1;
    assert!(!browser.selected_proton_uninstallable());
}

/// An in-flight install streams a status line into the overlay title.
#[test]
fn proton_overlay_shows_install_status() {
    isolate_config();
    let mut browser = Browser::new(vec![game(1, "Alpha", true, false)]);
    browser.protons = vec![proton("GE-Proton11-1", "Proton-GE", false)];
    *browser.proton_status.lock().unwrap() =
        Some(aurelia_tui::interface::game_status::GameStatus::msg(
            &None,
            "downloading 42.0%",
        ));
    assert!(
        render_proton(&browser).contains("downloading 42.0%"),
        "streamed install status appears in the overlay"
    );
}

fn workshop_item(id: u64, title: &str, subscribed: bool) -> WorkshopItemJson {
    WorkshopItemJson {
        id: Some(id),
        title: title.to_string(),
        subscribed,
        ..Default::default()
    }
}

/// Leaving browse mode and closing the overlay reset the browse state. (We set
/// the flags directly rather than via `open_workshop`/`workshop_enter_browse`,
/// which would shell out to the CLI.)
#[test]
fn workshop_browse_mode_toggles() {
    isolate_config();
    let mut browser = Browser::new(vec![game(1, "Alpha", true, false)]);
    browser.show_workshop = true;
    browser.workshop_browse = true;
    browser.workshop_query = "x".to_string();

    browser.workshop_exit_browse();
    assert!(!browser.workshop_browse, "back to the subscribed list");

    browser.close_workshop();
    assert!(!browser.show_workshop, "overlay closed");
    assert!(!browser.workshop_browse, "browse state reset on close");
    assert!(browser.workshop_query.is_empty(), "query cleared on close");
}

/// Highlight navigation over the browse results is clamped at both ends.
#[test]
fn workshop_result_navigation_clamps() {
    isolate_config();
    let mut browser = Browser::new(vec![game(1, "Alpha", true, false)]);
    browser.show_workshop = true;
    browser.workshop_browse = true;
    browser.workshop_results = vec![
        workshop_item(10, "Mod A", false),
        workshop_item(20, "Mod B", true),
    ];
    browser.workshop_index = 0;

    browser.workshop_previous();
    assert_eq!(browser.workshop_index, 0, "cannot go above the first row");

    browser.workshop_next();
    assert_eq!(browser.workshop_index, 1, "moved to the second row");
    browser.workshop_next();
    assert_eq!(browser.workshop_index, 1, "cannot go past the last row");

    let sel = browser.selected_workshop_result().expect("a row is selected");
    assert_eq!(sel.id, Some(20));
}

/// The browse pane renders the query, a result count, and the rows with their
/// subscribed tags and the highlight marker.
#[test]
fn workshop_browse_pane_renders() {
    isolate_config();
    let mut browser = Browser::new(vec![game(1, "Alpha", true, false)]);
    browser.show_workshop = true;
    browser.workshop_browse = true;
    browser.workshop_query = "aim".to_string();
    browser.workshop_results = vec![
        workshop_item(10, "AimTrainer", false),
        workshop_item(20, "BotMap", true),
    ];
    browser.workshop_index = 1;

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            f.render_widget(ui::workshop::workshop(&browser), Rect::new(0, 0, 80, 24));
        })
        .unwrap();

    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol.clone())
        .collect();

    assert!(text.contains("aim"), "query text is shown");
    assert!(text.contains("AimTrainer"), "first result title shown");
    assert!(text.contains("BotMap"), "second result title shown");
    assert!(text.contains("subscribed"), "subscribed tag shown");
    assert!(text.contains("browse"), "browse-mode title shown");
}

/// `workshop list` rows omit `subscribed` (the CLI only ever returns subscribed
/// items), so the struct default must be `true` — the list-path invariant that
/// `workshop_browse`'s explicit "force false" override depends on. Browse rows
/// (which never carry the field) are forced to `false` inside the parse fn, so
/// here we only pin the struct's own default behaviour both ways.
#[test]
fn workshop_item_subscribed_default() {
    // Absent field -> defaults to subscribed (the `workshop list` contract).
    let listed: WorkshopItemJson =
        serde_json::from_str(r#"{ "id": 3733868922, "title": "Mod" }"#).unwrap();
    assert!(
        listed.subscribed,
        "an item with no `subscribed` field defaults to subscribed (list path)"
    );

    // An explicit `false` (or the browse-path override) is honoured verbatim.
    let unlisted: WorkshopItemJson =
        serde_json::from_str(r#"{ "id": 42, "title": "X", "subscribed": false }"#).unwrap();
    assert!(
        !unlisted.subscribed,
        "an explicit false is preserved (browse rows are forced false)"
    );
}

/// Opening the comments sub-pane and exiting browse both reset its state. We set
/// the flags directly rather than via `workshop_open_comments` (which would
/// shell out to the CLI on a worker), to keep the test offline.
#[test]
fn workshop_comments_state_resets() {
    isolate_config();
    let mut browser = Browser::new(vec![game(1, "Alpha", true, false)]);
    browser.show_workshop = true;
    browser.workshop_browse = true;
    browser.workshop_comments_open = true;
    browser.workshop_comments = vec![WorkshopCommentJson {
        author: "Ada".to_string(),
        message: "great mod".to_string(),
        ..Default::default()
    }];
    browser.workshop_comments_scroll = 0;

    // Scroll is clamped to the single row (cannot advance past the last).
    browser.workshop_comments_scroll_down();
    assert_eq!(
        browser.workshop_comments_scroll, 0,
        "scroll clamps on a single comment"
    );

    browser.close_workshop_comments();
    assert!(
        !browser.workshop_comments_open,
        "sub-pane closed and reset"
    );
    assert!(
        browser.workshop_comments.is_empty(),
        "comments dropped on close"
    );

    // Exiting browse also tears the sub-pane down.
    browser.workshop_comments_open = true;
    browser.workshop_exit_browse();
    assert!(
        !browser.workshop_comments_open,
        "leaving browse closes the comments sub-pane"
    );
}

/// The comments sub-pane renders the item id in the title, a comment count, and
/// each comment's author header and body.
#[test]
fn workshop_comments_pane_renders() {
    isolate_config();
    let mut browser = Browser::new(vec![game(1, "Alpha", true, false)]);
    browser.show_workshop = true;
    browser.workshop_browse = true;
    browser.workshop_comments_open = true;
    browser.workshop_comments = vec![
        WorkshopCommentJson {
            author: "Ada".to_string(),
            message: "great mod".to_string(),
            ..Default::default()
        },
        WorkshopCommentJson {
            author: "Grace".to_string(),
            message: "crashes on load".to_string(),
            ..Default::default()
        },
    ];

    let backend = TestBackend::new(80, 24);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            f.render_widget(ui::workshop::workshop(&browser), Rect::new(0, 0, 80, 24));
        })
        .unwrap();

    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol.clone())
        .collect();

    assert!(text.contains("Comments"), "comments-pane title shown");
    assert!(text.contains("Ada"), "first author shown");
    assert!(text.contains("great mod"), "first comment body shown");
    assert!(text.contains("Grace"), "second author shown");
    assert!(text.contains("2 comment"), "comment count shown");
}

/// `workshop comments` rows parse leniently: aliases for the author/body keys
/// are accepted and a missing author falls back to a placeholder.
#[test]
fn workshop_comment_parses_aliases() {
    let c: WorkshopCommentJson =
        serde_json::from_str(r#"{ "name": "Ada", "text": "hi", "time": 42 }"#).unwrap();
    assert_eq!(c.author, "Ada");
    assert_eq!(c.message, "hi");
    assert_eq!(c.timestamp, 42);

    let anon: WorkshopCommentJson = serde_json::from_str(r#"{ "body": "no name" }"#).unwrap();
    assert_eq!(anon.display_author(), "(anonymous)");
    assert_eq!(anon.message, "no name");
}
