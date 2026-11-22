//! Capabilities, binary choice, account/logout fixtures.

use aurelia_tui::interface::aurelia::AccountJson;
use aurelia_tui::interface::binary::{self, BinarySignature, DaemonAction};
use aurelia_tui::interface::capabilities::{self, Capabilities, MUST_HAVE};
use aurelia_tui::interface::session::LogoutJson;

fn fixture(path: &str) -> String {
    let full = format!("{}/tests/fixtures/{path}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(full).unwrap()
}

fn caps() -> Capabilities {
    Capabilities::parse(&fixture("capabilities/v0.3.1.json")).unwrap()
}

#[test]
fn capabilities_fixture_parses() {
    let c = caps();
    assert_eq!(c.name, "aurelia");
    assert_eq!(c.version, "0.3.1");
    assert_eq!(c.schema, 1);
    assert!(c.commands.contains_key("account"));
    assert!(c.global_flags.iter().any(|f| f == "--json"));
}

#[test]
fn has_checks_features_commands_flags_events() {
    let c = caps();
    assert!(c.has("login.qr.stdin_control"));
    assert!(c.has("result.schema"));
    assert!(!c.has("login.qr.telepathy"));
    assert!(c.has("daemon stop"));
    assert!(c.has("daemon --socket"));
    assert!(c.has("config game --steam-runtime"));
    assert!(c.has("config game --json"), "global flags count");
    assert!(!c.has("config game --nope"));
    assert!(!c.has("daemon restart"));
    assert!(!c.has("nonexistent"));
    assert!(c.has("event:qr_challenge"));
    assert!(!c.has("event:nope"));
    assert!(!c.has(""));
}

#[test]
fn real_build_has_every_must_have() {
    assert!(caps().missing().is_empty(), "{:?}", caps().missing());
    let mut doc: serde_json::Value =
        serde_json::from_str(&fixture("capabilities/v0.3.1.json")).unwrap();
    doc["features"] = serde_json::json!([]);
    doc["commands"].as_object_mut().unwrap().remove("logout");
    let old: Capabilities = serde_json::from_value(doc).unwrap();
    let missing = old.missing();
    assert!(missing.contains(&"logout"));
    assert!(missing.contains(&"login.qr.stdin_control"));
    assert!(missing.len() < MUST_HAVE.len());
}

#[test]
fn global_probe_state_and_badge() {
    use tui::Terminal;
    use tui::backend::TestBackend;
    use tui::layout::Rect;
    let render = |version: Option<&str>| -> String {
        let mut terminal = Terminal::new(TestBackend::new(120, 1)).unwrap();
        terminal
            .draw(|f| {
                f.render_widget(
                    aurelia_tui::ui::status::version_badge(version),
                    Rect::new(0, 0, 120, 1),
                )
            })
            .unwrap();
        let buf = terminal.backend().buffer();
        (0..120).map(|x| buf.get(x, 0).symbol.clone()).collect()
    };

    capabilities::set_for_test(Some("0.3.1"), None);
    assert!(!capabilities::has("account"), "unprobed answers false");
    assert!(capabilities::missing().is_empty());

    capabilities::set_for_test(Some("0.3.1"), Some(caps()));
    assert_eq!(capabilities::version().as_deref(), Some("0.3.1"));
    assert!(capabilities::has("account"));
    assert!(render(Some("0.3.1")).trim_end().ends_with("aurelia v0.3.1"));

    let mut lacking = caps();
    lacking.commands.remove("logout");
    capabilities::set_for_test(Some("0.3.1"), Some(lacking));
    assert!(render(Some("0.3.1")).contains("(missing: logout)"));
    capabilities::set_for_test(None, None);
}

fn sig(path: &str, size: u64, mtime: u64) -> BinarySignature {
    BinarySignature {
        path: path.into(),
        size,
        mtime,
    }
}

#[test]
fn daemon_action_acts_only_on_change() {
    let a = sig("/usr/bin/aurelia", 10, 100);
    let rebuilt = sig("/usr/bin/aurelia", 11, 200);
    let other = sig("/opt/aurelia", 10, 100);
    assert_eq!(
        binary::daemon_action(None, &a, Some("0.3.1")),
        DaemonAction::Nothing
    );
    assert_eq!(
        binary::daemon_action(Some(&a), &a, Some("0.3.1")),
        DaemonAction::Nothing
    );
    assert_eq!(
        binary::daemon_action(Some(&a), &rebuilt, Some("0.3.1")),
        DaemonAction::Notice
    );
    assert_eq!(
        binary::daemon_action(Some(&a), &other, Some("0.3.1")),
        DaemonAction::Notice
    );
    assert_eq!(
        binary::daemon_action(Some(&a), &other, Some("0.1.34")),
        DaemonAction::Stop
    );
    assert_eq!(
        binary::daemon_action(Some(&a), &other, None),
        DaemonAction::Nothing
    );
}

#[test]
fn self_restart_threshold() {
    assert!(binary::self_restarts("0.1.35"));
    assert!(binary::self_restarts("v0.3.1-2"));
    assert!(!binary::self_restarts("0.1.34"));
    assert!(!binary::self_restarts("junk"));
}

#[test]
fn signature_tracks_size_and_path() {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("aurelia-tui-sig");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("aurelia");
    std::fs::write(&file, b"one").unwrap();
    let first = binary::signature(file.to_str().unwrap()).unwrap();
    assert_eq!(first.size, 3);
    assert!(first.path.ends_with("aurelia"));
    std::fs::write(&file, b"longer").unwrap();
    let second = binary::signature(file.to_str().unwrap()).unwrap();
    assert_ne!(first, second);
    assert!(binary::signature(dir.join("missing").to_str().unwrap()).is_none());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn config_keeps_binary_fields() {
    use aurelia_tui::config::Config;
    let mut legacy = serde_json::to_value(Config::default()).unwrap();
    legacy.as_object_mut().unwrap().remove("aurelia_bin");
    legacy.as_object_mut().unwrap().remove("last_binary");
    let config: Config = serde_json::from_value(legacy).unwrap();
    assert!(config.aurelia_bin.is_none() && config.last_binary.is_none());
    let chosen = Config {
        aurelia_bin: Some("~/bin/aurelia".into()),
        last_binary: Some(sig("/x", 1, 2)),
        ..config
    };
    let back: Config = serde_json::from_str(&serde_json::to_string(&chosen).unwrap()).unwrap();
    assert_eq!(back.aurelia_bin.as_deref(), Some("~/bin/aurelia"));
    assert_eq!(back.last_binary, Some(sig("/x", 1, 2)));
}

#[test]
fn account_reports_daemon_logon_id() {
    let daemon: AccountJson =
        serde_json::from_str(&fixture("session/account_daemon.json")).unwrap();
    assert_eq!(daemon.daemon_logon_id, Some(2864434397));
    let local: AccountJson =
        serde_json::from_str(&fixture("session/account_standalone.json")).unwrap();
    assert_eq!(local.daemon_logon_id, None);
}

#[test]
fn logout_summaries() {
    let parse = |p: &str| -> LogoutJson { serde_json::from_str(&fixture(p)).unwrap() };
    let revoked = parse("session/logout_revoked.json");
    assert!(revoked.logged_out);
    assert!(revoked.summary().contains("revoked at Steam"));
    assert!(
        parse("session/logout_none.json")
            .summary()
            .contains("no refresh token")
    );
    let failed = parse("session/logout_not_revoked.json");
    assert_eq!(failed.token_error.as_deref(), Some("timed out"));
    assert!(
        failed
            .summary()
            .contains("did not revoke the token: timed out")
    );
    let old: LogoutJson = serde_json::from_str(r#"{"logged_out":true}"#).unwrap();
    assert_eq!(old.summary(), "Logged out.");
}
