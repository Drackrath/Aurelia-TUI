//! Renders the Friends panel and the Chat widget into tui's off-screen
//! `TestBackend` and asserts the optimized behaviour:
//! - Friends: always-visible panel, focus-aware title, whole-list scroll that
//!   keeps the highlight visible.
//! - Chat: newest message anchored at the BOTTOM, own messages RIGHT-aligned,
//!   partner messages LEFT-aligned, with a composer input line.
//!
//! The `*_screenshot` tests dump the rendered buffer as text; run them with
//! `cargo test --test friends_chat -- --nocapture` to eyeball the layout.

use aurelia_tui::browse::Browser;
use aurelia_tui::interface::aurelia::{ChatMessageJson, FriendJson, FriendSearchJson, LibraryGameJson};
use aurelia_tui::interface::game::Game;
use aurelia_tui::ui;
use tui::backend::TestBackend;
use tui::layout::{Constraint, Direction, Layout, Rect};
use tui::Terminal;

fn isolate_config() {
    let dir = std::env::temp_dir().join("aurelia-tui-test-config");
    unsafe {
        std::env::set_var("AURELIA_TUI_DIR", &dir);
        std::env::set_var("AURELIA_TUI_CACHE_DIR", &dir);
    }
}

fn friend(id: u64, name: &str, state: u32, game: Option<&str>) -> FriendJson {
    FriendJson {
        steam_id: id,
        persona_name: Some(name.to_string()),
        persona_state: Some(state),
        game_app_id: None,
        game_name: game.map(|g| g.to_string()),
    }
}

fn msg(text: &str, from_self: bool) -> ChatMessageJson {
    ChatMessageJson {
        sender: if from_self { 0 } else { 1 },
        from_self,
        message: text.to_string(),
        timestamp: 0,
    }
}

/// Render `draw` into a `w`x`h` TestBackend and return the buffer as one string
/// per row.
fn render_rows<F: FnOnce(&mut tui::Frame<TestBackend>)>(w: u16, h: u16, draw: F) -> Vec<String> {
    let backend = TestBackend::new(w, h);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal.draw(|f| draw(f)).unwrap();
    let buf = terminal.backend().buffer().clone();
    (0..h)
        .map(|y| (0..w).map(|x| buf.get(x, y).symbol.clone()).collect::<String>())
        .collect()
}

fn print_shot(label: &str, rows: &[String]) {
    println!("\n=== {label} ===");
    for r in rows {
        println!("{}", r.trim_end());
    }
}

#[test]
fn friends_panel_focus_aware_and_highlights() {
    isolate_config();
    let mut b = Browser::new(vec![]);
    b.friends = vec![
        friend(1, "Alice", 1, Some("Dota 2")),
        friend(2, "Bob", 0, None),
        friend(3, "Carol", 1, None),
    ];
    b.friends_index = 0;

    // Unfocused: calmer title, no selection background (marker still present).
    b.friends_focused = false;
    let unfocused = render_rows(40, 8, |f| {
        f.render_widget(ui::friends::friends(&b, 6), Rect::new(0, 0, 40, 8));
    });
    let joined = unfocused.join("\n");
    assert!(joined.contains("Friends (3)"), "shows friend count");
    assert!(joined.contains("[F] focus"), "calm title hints how to focus");
    assert!(joined.contains("Alice"), "lists a friend");
    assert!(joined.contains("Dota 2"), "shows the current game");
    print_shot("Friends (unfocused)", &unfocused);

    // Focused: louder title advertising chat + new-window shortcuts.
    b.friends_focused = true;
    let focused = render_rows(40, 8, |f| {
        f.render_widget(ui::friends::friends(&b, 6), Rect::new(0, 0, 40, 8));
    });
    let joined = focused.join("\n");
    assert!(joined.contains("chat"), "focused title advertises chat");
    assert!(joined.contains("window"), "focused title advertises [t] window");
    assert!(joined.contains("\u{25b6}"), "highlighted row shows the ▶ marker");
    print_shot("Friends (focused)", &focused);
}

#[test]
fn friends_whole_list_scrolls_to_keep_highlight_visible() {
    isolate_config();
    let mut b = Browser::new(vec![]);
    b.friends = (0..10)
        .map(|i| friend(i as u64, &format!("Friend{i}"), 1, None))
        .collect();
    b.friends_focused = true;

    // Only 3 content rows visible. Highlight the 8th friend: the window must
    // scroll so Friend7 is visible and the early ones are scrolled off.
    b.friends_index = 7;
    let rows = render_rows(30, 5, |f| {
        f.render_widget(ui::friends::friends(&b, 3), Rect::new(0, 0, 30, 5));
    });
    let joined = rows.join("\n");
    assert!(joined.contains("Friend7"), "highlighted friend is visible");
    assert!(!joined.contains("Friend0"), "early friends scrolled off the top");
    print_shot("Friends (scrolled to #7)", &rows);
}

#[test]
fn chat_bottom_anchored_with_per_side_alignment() {
    isolate_config();
    let mut b = Browser::new(vec![]);
    b.chat_partner = "Alice".to_string();
    b.chat_input = "typing...".to_string();
    b.chat_messages = vec![
        msg("hello there", false),
        msg("hi yourself", true),
    ];

    let w = 50u16;
    let h = 14u16;
    let rows = render_rows(w, h, |f| {
        f.render_widget(ui::chat::chat(&b, w, h), Rect::new(0, 0, w, h));
    });
    print_shot("Chat", &rows);

    let other_row = rows.iter().position(|r| r.contains("hello there")).expect("partner msg");
    let own_row = rows.iter().position(|r| r.contains("hi yourself")).expect("own msg");

    // Newest (own) message sits BELOW the earlier partner message.
    assert!(own_row > other_row, "newest message is anchored lower (bottom)");

    // Partner message LEFT-aligned, own message RIGHT-aligned: the own text must
    // start in a much later column than the partner text.
    let other_col = rows[other_row].find("hello there").unwrap();
    let own_col = rows[own_row].find("hi yourself").unwrap();
    assert!(
        own_col > other_col + 10,
        "own message right-aligned (own col {own_col}) vs partner left (col {other_col})"
    );

    // Both messages hug the bottom: they are in the lower half of the panel.
    assert!(other_row >= (h as usize) / 2, "messages bottom-anchored, not at top");

    // Composer present: a separator row and the input line with the caret.
    let joined = rows.join("\n");
    assert!(joined.contains("typing..."), "composer shows the in-progress input");
    assert!(joined.contains('\u{2500}'), "a separator divides history from input");
    assert!(joined.contains("me:"), "own messages are labelled");
    assert!(joined.contains("Alice:"), "partner messages are labelled");
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
    })
}

/// Mirror `main.rs`'s right pane: split into Detail (top) over the always-visible
/// Friends panel (bottom). Proves both sections render, ordered, in one frame.
#[test]
fn right_pane_splits_detail_over_friends() {
    isolate_config();
    let mut b = Browser::new(vec![game(1, "Alpha")]);
    b.friends = vec![friend(1, "Alice", 1, Some("Dota 2"))];

    let w = 60u16;
    let h = 24u16;
    let rows = render_rows(w, h, |f| {
        let right = Rect::new(0, 0, w, h);
        let friends_h = (right.height / 3).clamp(6, 16);
        let split = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(6), Constraint::Length(friends_h)])
            .split(right);
        let selected = b.selected();
        f.render_widget(
            ui::detail::detail(selected.as_ref(), false, 40, split[0].height.saturating_sub(2)),
            split[0],
        );
        let fr_rows = split[1].height.saturating_sub(2) as usize;
        f.render_widget(ui::friends::friends(&b, fr_rows), split[1]);
    });
    print_shot("Right pane (detail over friends)", &rows);

    let detail_row = rows.iter().position(|r| r.contains("Detail")).expect("detail panel");
    let friends_row = rows.iter().position(|r| r.contains("Friends")).expect("friends panel");
    assert!(detail_row < friends_row, "Detail sits above Friends");
    assert!(rows.join("\n").contains("Alice"), "friend listed in the panel");
}

#[test]
fn friend_add_overlay_shows_input_and_resolved_preview() {
    isolate_config();
    let mut b = Browser::new(vec![]);

    // Typed query, no search yet: the labelled input is shown with a caret.
    b.show_friend_add = true;
    b.friend_add_query = "gabelogannewell".to_string();
    let rows = render_rows(70, 10, |f| {
        f.render_widget(ui::friend_add::friend_add_overlay(&b), Rect::new(0, 0, 70, 10));
    });
    let joined = rows.join("\n");
    assert!(joined.contains("Add friend"), "overlay titled");
    assert!(joined.contains("vanity"), "label names the accepted reference forms");
    assert!(joined.contains("gabelogannewell"), "shows the typed query");
    assert!(joined.contains("[Enter] search"), "title hints the search key");
    assert!(joined.contains("[a] send request"), "title hints the add key");
    print_shot("Add friend (input)", &rows);

    // After a successful `friends search`, a resolved-account preview appears.
    b.friend_search_result = Some(FriendSearchJson {
        steam_id: 76561197960287930,
        persona_name: Some("Rabscuttle".to_string()),
        profile_url: None,
    });
    if let Ok(mut status) = b.friend_add_status.lock() {
        *status = "Found Rabscuttle".to_string();
    }
    let rows = render_rows(70, 12, |f| {
        f.render_widget(ui::friend_add::friend_add_overlay(&b), Rect::new(0, 0, 70, 12));
    });
    let joined = rows.join("\n");
    assert!(joined.contains("Resolved:"), "resolved preview present");
    assert!(joined.contains("Rabscuttle"), "preview names the account");
    assert!(joined.contains("76561197960287930"), "preview shows the SteamID");
    assert!(joined.contains("send the friend request"), "preview hints the confirm key");
    print_shot("Add friend (resolved)", &rows);
}

#[test]
fn chat_overflow_keeps_newest() {
    isolate_config();
    let mut b = Browser::new(vec![]);
    b.chat_partner = "Alice".to_string();
    // More messages than rows: oldest must be dropped, newest kept.
    b.chat_messages = (0..20).map(|i| msg(&format!("msg{i}"), i % 2 == 0)).collect();

    let w = 40u16;
    let h = 10u16; // small: forces overflow
    let rows = render_rows(w, h, |f| {
        f.render_widget(ui::chat::chat(&b, w, h), Rect::new(0, 0, w, h));
    });
    let joined = rows.join("\n");
    assert!(joined.contains("msg19"), "newest message kept");
    assert!(!joined.contains("msg0"), "oldest dropped");
    print_shot("Chat (overflow keeps newest)", &rows);
}
