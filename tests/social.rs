//! Social features against Aurelia fixtures.

use aurelia_tui::browse::{Browser, View};
use aurelia_tui::interface::aurelia::{ChatMessageJson, ConfigJson, FriendJson};
use aurelia_tui::interface::social::{
    self, InviteSentJson, PersonaWaitJson, UserProfileJson, chat_display_text, invite_args,
    parse_persona_wait, persona_state_label,
};
use aurelia_tui::social::InviteField;
use aurelia_tui::ui;
use tui::Terminal;
use tui::backend::TestBackend;
use tui::layout::Rect;

fn isolate_config() {
    let dir = std::env::temp_dir().join("aurelia-tui-test-config");
    unsafe {
        std::env::set_var("AURELIA_TUI_DIR", &dir);
        std::env::set_var("AURELIA_TUI_CACHE_DIR", &dir);
        std::env::set_var("AURELIA_BIN", dir.join("missing-backend"));
    }
}

fn fixture(name: &str) -> String {
    let path = format!(
        "{}/tests/fixtures/social/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn render(w: u16, h: u16, draw: impl FnOnce(&mut tui::Frame<TestBackend>)) -> String {
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(draw).unwrap();
    let buf = terminal.backend().buffer().clone();
    (0..h)
        .map(|y| {
            (0..w)
                .map(|x| buf.get(x, y).symbol.clone())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn status_labels_match_aurelia() {
    assert_eq!(persona_state_label(None), "offline");
    assert_eq!(persona_state_label(Some(0)), "offline");
    assert_eq!(persona_state_label(Some(3)), "away");
    assert_eq!(persona_state_label(Some(6)), "looking to play");
    assert_eq!(persona_state_label(Some(7)), "invisible");
    assert_eq!(persona_state_label(Some(42)), "online");
}

#[test]
fn friends_fixture_parses_statuses() {
    let friends: Vec<FriendJson> = serde_json::from_str(&fixture("friends.json")).unwrap();
    assert_eq!(friends.len(), 4);
    assert!(friends[0].is_online(), "away counts as online");
    assert!(!friends[1].is_online(), "missing status is offline");
    assert_eq!(
        friends[2].current_game().as_deref(),
        Some("Team Fortress 2")
    );
    assert!(!friends[3].is_online(), "invisible is not online");
}

#[test]
fn friends_panel_shows_status_labels() {
    isolate_config();
    let mut b = Browser::new(vec![]);
    b.friends = serde_json::from_str(&fixture("friends.json")).unwrap();
    b.view = View::Friends;
    let out = render(80, 8, |f| {
        f.render_widget(ui::friends::friends(&b, 6), Rect::new(0, 0, 80, 8));
    });
    assert!(out.contains("Away Friend (away)"), "{out}");
    assert!(out.contains("Ghost (invisible)"), "{out}");
    assert!(!out.contains("Player (online)"), "{out}");
    assert!(out.contains("[i] invite"), "{out}");
}

#[test]
fn user_profile_fixture_parses() {
    let p: UserProfileJson = serde_json::from_str(&fixture("user_profile.json")).unwrap();
    assert_eq!(p.steam_id, 76561197960287930);
    assert_eq!(p.display_name(), "Rabscuttle");
    assert_eq!(p.status_label(), "away");
    assert_eq!(p.visibility_label(), "public");
    assert_eq!(p.playing().as_deref(), Some("Team Fortress 2"));
    assert_eq!(p.sources.len(), 3);
}

#[test]
fn own_profile_shows_invisible() {
    let p: UserProfileJson = serde_json::from_str(&fixture("user_me_invisible.json")).unwrap();
    assert_eq!(p.status_label(), "invisible");
    assert_eq!(p.visibility_label(), "private");
    let text: String = ui::social::profile_lines(&p)
        .iter()
        .map(|l| {
            l.0.iter()
                .map(|s| s.content.to_string())
                .collect::<String>()
                + "\n"
        })
        .collect();
    assert!(text.contains("invisible"), "{text}");
    assert!(text.contains("limited account"), "{text}");
}

#[test]
fn profile_lines_cover_fields() {
    let p: UserProfileJson = serde_json::from_str(&fixture("user_profile.json")).unwrap();
    let text: String = ui::social::profile_lines(&p)
        .iter()
        .map(|l| {
            l.0.iter()
                .map(|s| s.content.to_string())
                .collect::<String>()
                + "\n"
        })
        .collect();
    assert!(text.contains("Playing   Team Fortress 2"), "{text}");
    assert!(text.contains("Created   2003-09-12"), "{text}");
    assert!(text.contains("Last seen 2023-11-14"), "{text}");
    assert!(text.contains("Hello there& welcome\nsecond line"), "{text}");
}

#[test]
fn invite_json_and_args() {
    let sent: InviteSentJson = serde_json::from_str(&fixture("chat_invite.json")).unwrap();
    assert!(sent.sent);
    assert_eq!(sent.app_id, 440);
    assert_eq!(
        invite_args(5, 440, None),
        ["chat", "invite", "5", "--app", "440"]
    );
    assert_eq!(
        invite_args(5, 440, Some("  ")),
        ["chat", "invite", "5", "--app", "440"]
    );
    assert_eq!(
        invite_args(5, 440, Some("+connect 1.2.3.4")),
        [
            "chat",
            "invite",
            "5",
            "--app",
            "440",
            "--connect",
            "+connect 1.2.3.4"
        ]
    );
}

#[test]
fn persona_wait_json_and_validation() {
    let wait: PersonaWaitJson = serde_json::from_str(&fixture("persona_wait.json")).unwrap();
    assert_eq!(wait.persona_wait_secs, Some(45));
    let config: ConfigJson = serde_json::from_str(&fixture("config_show.json")).unwrap();
    assert_eq!(config.persona_wait_secs, Some(45));
    assert!(!config.is_online());
    assert_eq!(parse_persona_wait("30"), Ok(30));
    assert_eq!(parse_persona_wait(" 300 "), Ok(300));
    assert!(parse_persona_wait("0").is_err());
    assert!(parse_persona_wait("301").is_err());
    assert!(parse_persona_wait("x").is_err());
    assert_eq!(social::PERSONA_WAIT_DEFAULT, 30);
}

#[test]
fn settings_show_persona_wait_row() {
    isolate_config();
    let mut b = Browser::new(vec![]);
    b.config_info = Some(serde_json::from_str(&fixture("config_show.json")).unwrap());
    let out = render(100, 20, |f| {
        f.render_widget(ui::config::config(&b), Rect::new(0, 0, 100, 20));
    });
    assert!(out.contains("Persona wait"), "{out}");
    assert!(out.contains("45s"), "{out}");
    assert!(out.contains("offline (invisible)"), "{out}");

    b.begin_persona_wait_edit();
    assert_eq!(b.social.wait_input.as_deref(), Some("45"));
    b.persona_wait_pop();
    b.persona_wait_push('x');
    b.persona_wait_push('0');
    assert_eq!(b.social.wait_input.as_deref(), Some("40"));
    let out = render(100, 20, |f| {
        f.render_widget(ui::config::config(&b), Rect::new(0, 0, 100, 20));
    });
    assert!(out.contains("40_ s"), "{out}");
    b.cancel_persona_wait_edit();
    assert!(b.social.wait_input.is_none());
}

#[test]
fn history_invites_render_readably() {
    let history: Vec<ChatMessageJson> =
        serde_json::from_str(&fixture("chat_history_invite.json")).unwrap();
    assert_eq!(
        chat_display_text(&history[0].message),
        "* sent a game invite (app 440)"
    );
    assert_eq!(chat_display_text(&history[1].message), "see you there");
    assert_eq!(
        chat_display_text("[lobbyinvite lobbyid=\"1\"][/lobbyinvite]"),
        "* sent a game invite"
    );
}

#[test]
fn invite_overlay_flow() {
    isolate_config();
    let mut b = Browser::new(vec![]);
    b.friends = serde_json::from_str(&fixture("friends.json")).unwrap();
    b.view = View::Friends;
    b.open_invite();
    let draft = b.social.invite.as_ref().expect("invite opens");
    assert_eq!(draft.steamid, 76561197960287930);
    assert_eq!(draft.field, InviteField::App);

    for c in "44a0".chars() {
        b.invite_push(c);
    }
    b.invite_toggle_field();
    for c in "+connect x".chars() {
        b.invite_push(c);
    }
    let draft = b.social.invite.as_ref().unwrap();
    assert_eq!(draft.app_input, "440");
    assert_eq!(draft.connect, "+connect x");

    let out = render(90, 14, |f| {
        f.render_widget(
            ui::social::invite_overlay(b.social.invite.as_ref().unwrap()),
            Rect::new(0, 0, 90, 14),
        );
    });
    assert!(out.contains("Invite Away Friend"), "{out}");
    assert!(out.contains("440"), "{out}");
    assert!(out.contains("+connect x"), "{out}");

    // Empty app id is refused locally.
    b.invite_toggle_field();
    for _ in 0..3 {
        b.invite_pop();
    }
    b.invite_send();
    let status = b.social.invite.as_ref().unwrap().status_text();
    assert!(status.contains("app id"), "{status}");
    b.close_invite();
    assert!(b.social.invite.is_none());
}
