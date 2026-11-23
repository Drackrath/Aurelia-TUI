//! Friends fixes: names, clans, hints, overlays.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use aurelia_tui::browse::{Browser, View};
use aurelia_tui::interface::aurelia::FriendJson;
use aurelia_tui::interface::social::UserProfileJson;
use aurelia_tui::social::names::{self, NAV_IDLE, is_individual};
use aurelia_tui::social::{InviteDraft, ProfileView};
use aurelia_tui::ui;
use tui::Terminal;
use tui::backend::TestBackend;
use tui::layout::Rect;

const CLAN: u64 = 103582791441395743;
const SIZES: [(u16, u16); 2] = [(120, 40), (80, 24)];

fn isolate_config() {
    let dir = std::env::temp_dir().join("aurelia-tui-test-config");
    unsafe {
        std::env::set_var("AURELIA_TUI_DIR", &dir);
        std::env::set_var("AURELIA_TUI_CACHE_DIR", &dir);
        std::env::set_var("AURELIA_BIN", dir.join("missing-backend"));
    }
}

fn unnamed(id: u64) -> FriendJson {
    FriendJson {
        steam_id: id,
        ..Default::default()
    }
}

/// 27 unnamed friends plus clan.
fn roster() -> Vec<FriendJson> {
    let mut list: Vec<FriendJson> = (0..27).map(|i| unnamed(76561197985686146 + i)).collect();
    list.push(unnamed(CLAN));
    list
}

fn render(w: u16, h: u16, draw: impl FnOnce(&mut tui::Frame<TestBackend>)) -> Vec<String> {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(draw).unwrap();
    let buf = terminal.backend().buffer().clone();
    (0..h)
        .map(|y| {
            (0..w)
                .map(|x| buf.get(x, y).symbol.clone())
                .collect::<String>()
        })
        .collect()
}

/// The left pane main.rs gives Friends.
fn friends_pane(w: u16, h: u16) -> Rect {
    let body_h = h - 3 - 2;
    Rect::new(0, 3, w * 40 / 100, body_h)
}

fn profile(id: u64, name: &str) -> UserProfileJson {
    UserProfileJson {
        steam_id: id,
        persona_name: Some(name.into()),
        persona_state: Some(0),
        ..Default::default()
    }
}

#[test]
fn account_type_bits() {
    assert!(is_individual(76561197985686146));
    assert!(!is_individual(CLAN));
}

#[test]
fn poll_drops_clans() {
    isolate_config();
    let mut b = Browser::new(vec![]);
    b.friends = roster();
    b.friends_index = 27;
    b.poll_social_at(Instant::now(), false);
    assert_eq!(b.friends.len(), 27);
    assert!(b.friends.iter().all(|f| f.steam_id != CLAN));
    assert_eq!(b.friends_index, 26);
}

#[test]
fn profile_name_is_written_back() {
    isolate_config();
    let mut b = Browser::new(vec![]);
    b.friends = roster();
    let id = b.friends[0].steam_id;
    b.social.profile = Some(ProfileView {
        ident: id.to_string(),
        label: id.to_string(),
        result: Arc::new(Mutex::new(Some(Ok(profile(id, "Magadanch1k"))))),
        ..Default::default()
    });
    b.poll_social_at(Instant::now(), false);
    assert_eq!(b.friends[0].display_name(), "Magadanch1k");
    assert_eq!(b.friends[0].persona_state, Some(0));

    // Cache survives a roster refresh.
    b.social.profile = None;
    b.friends = roster();
    b.poll_social_at(Instant::now(), false);
    assert_eq!(b.friends[0].display_name(), "Magadanch1k");
}

#[test]
fn resolver_is_idle_gated_and_single() {
    let mut r = names::NameResolver::default();
    let friends: Vec<FriendJson> = roster().into_iter().take(27).collect();
    r.visible_rows.set(5);
    let t0 = Instant::now();
    r.note_nav(0, true, t0);
    assert!(!r.lookup_due(t0), "not right after entering");
    assert!(r.lookup_due(t0 + NAV_IDLE));

    // Scrolling restarts the idle timer.
    r.note_nav(1, true, t0 + NAV_IDLE);
    assert!(!r.lookup_due(t0 + NAV_IDLE + Duration::from_secs(1)));

    // Only visible rows, untried ones first.
    assert_eq!(r.next_lookup(&friends, 20), Some(friends[16].steam_id));
    r.attempted.insert(friends[16].steam_id);
    assert_eq!(r.next_lookup(&friends, 20), Some(friends[17].steam_id));

    // One lookup in flight at most.
    r.inflight = Some((1, Arc::new(Mutex::new(None))));
    assert!(!r.lookup_due(t0 + Duration::from_secs(60)));
    r.inflight = None;

    // A gap after each lookup.
    let later = t0 + Duration::from_secs(60);
    r.last_done = Some(later);
    assert!(!r.lookup_due(later + Duration::from_secs(1)));
    assert!(r.lookup_due(later + names::LOOKUP_GAP));
}

#[test]
fn missing_names_note_and_scroll_at_both_sizes() {
    isolate_config();
    for (w, h) in SIZES {
        let mut b = Browser::new(vec![]);
        b.friends = roster();
        b.view = View::Friends;
        b.poll_social_at(Instant::now(), false);
        b.social.names.steam_here = true;
        // One name arrived; note stays.
        b.friends[0].persona_name = Some("Magadanch1k".into());
        b.friends_last();
        let pane = friends_pane(w, h);
        let rows = render(w, h, |f| {
            let n = pane.height.saturating_sub(2) as usize;
            f.render_widget(ui::friends::friends(&b, n), pane);
        });
        let out = rows.join("\n");
        assert!(out.contains("● Friends (27)"), "{w}x{h}\n{out}");
        assert!(
            out.contains("Names fill in by themselves"),
            "{w}x{h}\n{out}"
        );
        assert!(
            out.contains("26 left · close Steam for all"),
            "{w}x{h}\n{out}"
        );
        let last = b.friends.last().unwrap().steam_id.to_string();
        assert!(
            rows.iter().any(|r| r.contains('▶') && r.contains(&last)),
            "{w}x{h}: last row selected and visible\n{out}"
        );
        assert!(!out.contains(&CLAN.to_string()));
    }
}

#[test]
fn friends_footer_hints_fit_both_sizes() {
    isolate_config();
    for (w, h) in SIZES {
        let mut b = Browser::new(vec![]);
        b.view = View::Friends;
        let rows = render(w, h, |f| {
            f.render_widget(
                ui::status::status_bar(&b, Some("someone")),
                Rect::new(0, h - 2, w, 2),
            );
        });
        let hints = &rows[(h - 1) as usize];
        assert!(hints.contains("[Enter] chat"), "{hints}");
        assert!(hints.contains("[x] remove"), "{w}x{h}: {hints}");
        assert!(!hints.contains("[d] install"), "{hints}");
    }
}

#[test]
fn invite_overlay_header_and_title_fit() {
    isolate_config();
    for (w, h) in SIZES {
        let draft = InviteDraft {
            steamid: 76561197985686146,
            friend_name: "76561197985686146".into(),
            app_input: "1086940".into(),
            game_name: Some("Baldur's Gate 3".into()),
            ..Default::default()
        };
        let rows = render(w, h, |f| {
            let area = ui::centered_rect(60, 40, f.size());
            f.render_widget(ui::social::invite_overlay(&draft), area);
        });
        let out = rows.join("\n");
        assert!(
            out.contains("Invite · [Enter] send [Tab] field [Esc] close"),
            "{w}x{h}\n{out}"
        );
        assert_eq!(out.matches("76561197985686146").count(), 1, "{out}");
    }
    let named = InviteDraft {
        steamid: 5,
        friend_name: "Magadanch1k".into(),
        ..Default::default()
    };
    assert_eq!(named.header(), ("Magadanch1k".into(), Some("5".into())));
}

#[test]
fn invite_status_shows_elapsed() {
    let draft = InviteDraft::default();
    *draft.status.lock().unwrap() = "Inviting x to app 1…".into();
    let start = Instant::now();
    *draft.sent_at.lock().unwrap() = Some(start);
    assert_eq!(
        draft.status_text_at(start + Duration::from_secs(12)),
        "Inviting x to app 1… 12s"
    );
    *draft.sent_at.lock().unwrap() = None;
    assert_eq!(draft.status_text_at(start), "Inviting x to app 1…");
}

#[test]
fn profile_overlay_loading_and_title_fit() {
    for (w, h) in SIZES {
        let view = ProfileView {
            ident: "76561197985686146".into(),
            label: "76561197985686146".into(),
            started: Some(Instant::now() - Duration::from_secs(7)),
            ..Default::default()
        };
        assert_eq!(view.elapsed_secs(Instant::now()), 7);
        let rows = render(w, h, |f| {
            let area = ui::centered_rect(70, 70, f.size());
            f.render_widget(ui::social::profile_overlay(&view), area);
        });
        let out = rows.join("\n");
        assert!(
            out.contains("Profile · [j/k] scroll [Esc] close"),
            "{w}x{h}\n{out}"
        );
        assert!(out.contains("7s"), "{w}x{h}\n{out}");
    }
}

#[test]
fn note_clears_when_all_named() {
    let mut friends = roster();
    friends.pop();
    assert_eq!(names::missing_note(&friends, true).len(), 2);
    for f in friends.iter_mut() {
        f.persona_name = Some("x".into());
    }
    assert!(names::missing_note(&friends, true).is_empty());
    assert!(names::missing_note(&friends, false).is_empty());
}

#[test]
fn jump_and_page_keys_keep_row_visible() {
    isolate_config();
    for (w, h) in SIZES {
        let mut b = Browser::new(vec![]);
        b.friends = roster();
        b.view = View::Friends;
        b.poll_social_at(Instant::now(), false);
        b.social.names.steam_here = true;
        let pane = friends_pane(w, h);
        let n = pane.height.saturating_sub(2) as usize;
        let draw =
            |b: &Browser| render(w, h, |f| f.render_widget(ui::friends::friends(b, n), pane));
        draw(&b);
        let page = b.social.names.visible_rows.get();
        b.friends_page(true);
        assert_eq!(b.friends_index, page.min(26));
        b.friends_last();
        let rows = draw(&b);
        let last = b.friends[26].steam_id.to_string();
        assert!(rows.iter().any(|r| r.contains('▶') && r.contains(&last)));
        b.friends_page(true);
        assert_eq!(b.friends_index, 26, "page clamps at end");
        b.friends_page(false);
        assert_eq!(b.friends_index, 26usize.saturating_sub(page));
        b.friends_first();
        let rows = draw(&b);
        let first = b.friends[0].steam_id.to_string();
        assert!(rows.iter().any(|r| r.contains('▶') && r.contains(&first)));
    }
}
