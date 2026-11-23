//! Typed errors, login results, Aurelia log.

use std::fs;
use std::path::PathBuf;

use aurelia_tui::interface::aurelia_log::{self, LogSink};
use aurelia_tui::interface::{cli_json, session};
use aurelia_tui::ui::aurelia_log::LogView;
use aurelia_tui::util::error::{CliError, STError};

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/errors")
        .join(name);
    fs::read_to_string(path).unwrap()
}

fn cli_error(name: &str) -> STError {
    cli_json::parse(&fixture(name), Some(77)).unwrap_err()
}

#[test]
fn auth_required_carries_kind_and_hint() {
    let err = cli_error("auth_required.json");
    assert_eq!(err.kind(), Some("auth_required"));
    assert!(err.is_kind("auth_required"));
    assert_eq!(err.hint(), Some("run `aurelia login`"));
    assert_eq!(
        err.message(),
        "could not restore the stored session: stored refresh token was rejected"
    );
    assert_eq!(
        err.to_string(),
        "could not restore the stored session: stored refresh token was rejected — run `aurelia login`"
    );
    assert_eq!(err.cli().unwrap().exit_code, Some(77));
}

#[test]
fn rate_limited_keeps_retry_after() {
    let err = cli_error("rate_limited.json");
    let cli = err.cli().unwrap();
    assert!(cli.is("rate_limited"));
    assert_eq!(cli.retry_after_seconds, Some(60));
    assert!(
        err.to_string()
            .ends_with("— Steam is throttling this client; pause before retrying")
    );
}

#[test]
fn unknown_kind_without_hint_shows_message() {
    let err = cli_error("keyring_locked.json");
    assert_eq!(err.kind(), Some("unknown"));
    assert_eq!(err.hint(), None);
    assert_eq!(err.to_string(), err.message());
    let advice = session::session_advice(&err.to_string()).unwrap();
    assert!(advice.contains("keyring"));
}

#[test]
fn disk_full_is_typed() {
    let err = cli_error("disk_full.json");
    assert!(err.is_kind("disk_full"));
    assert!(
        err.to_string()
            .contains(" — free space on the target drive")
    );
}

#[test]
fn reconnect_hint_gets_tui_advice() {
    let err = cli_error("web_session_rejected.json");
    assert!(err.is_kind("auth_required"));
    let text = session::with_advice(err.to_string());
    assert!(text.contains("Reconnect Steam session"));
}

#[test]
fn hint_already_in_message_is_not_repeated() {
    let value =
        serde_json::json!({"error": "boom; run `aurelia login`", "hint": "run `aurelia login`"});
    let err = CliError::from_json(&value).unwrap();
    assert_eq!(err.summary(), "boom; run `aurelia login`");
}

#[test]
fn success_payloads_are_not_errors() {
    let value = cli_json::parse(&fixture("install_result.json"), Some(0))
        .unwrap()
        .unwrap();
    assert_eq!(value["status"], "installed");
    assert!(CliError::from_json(&serde_json::json!({"type": "approved"})).is_none());
}

#[test]
fn pretty_json_parses_as_one_value() {
    let value = cli_json::parse(&fixture("reconnect.json"), Some(0))
        .unwrap()
        .unwrap();
    let parsed: session::ReconnectJson = serde_json::from_value(value).unwrap();
    assert!(parsed.logged_in && parsed.daemon);
    assert_eq!(
        session::reconnect_notice(Ok(parsed)),
        "Steam session reconnected (gaben)"
    );
}

#[test]
fn standalone_reconnect_error_notice() {
    let err = cli_error("reconnect_standalone.json");
    let notice = session::reconnect_notice(Err(err));
    assert!(notice.starts_with("Reconnect failed: --reconnect needs the session daemon"));
}

#[test]
fn empty_stdout_is_none() {
    assert!(cli_json::parse("  \n", Some(1)).unwrap().is_none());
    assert_eq!(
        cli_json::last_stderr_line(b"one\nerror: unexpected argument\n\n").as_deref(),
        Some("error: unexpected argument")
    );
}

#[test]
fn qr_success_spans_two_lines() {
    assert_eq!(
        cli_json::values(&fixture("qr_success.ndjson"))
            .unwrap()
            .len(),
        2
    );
    assert!(session::qr_result(&fixture("qr_success.ndjson")).is_ok());
}

#[test]
fn qr_expired_is_typed() {
    let err = session::qr_result(&fixture("qr_expired.ndjson")).unwrap_err();
    assert!(err.is_kind("login_expired"));
    assert!(err.to_string().contains("— approve sooner after scanning"));
}

#[test]
fn qr_cancelled_and_empty() {
    let err = session::qr_result(&fixture("qr_cancelled.ndjson")).unwrap_err();
    assert!(err.is_kind("cancelled"));
    let err = session::qr_result("").unwrap_err();
    assert!(err.to_string().contains("no result"));
}

#[test]
fn classic_results() {
    assert!(session::classic_result(&fixture("classic_success.json")).is_ok());
    let err = session::classic_result(&fixture("classic_guard_failed.json")).unwrap_err();
    assert!(err.is_kind("auth_required"));
    assert!(
        err.to_string()
            .starts_with("login failed after providing Steam Guard code")
    );
    let err = session::classic_result("").unwrap_err();
    assert_eq!(err.to_string(), "login did not complete");
}

#[test]
fn problem_variant_unchanged() {
    let err = STError::Problem("plain".into());
    assert_eq!(err.kind(), None);
    assert_eq!(err.to_string(), "plain");
    assert_eq!(err.message(), "plain");
}

fn scratch(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("errors_log")
        .join(name);
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir.join("aurelia.log")
}

#[test]
fn keep_line_drops_progress_ticks() {
    let kept: Vec<String> = fixture("install_stderr.txt")
        .lines()
        .filter(|l| aurelia_log::keep_line(l))
        .map(str::to_string)
        .collect();
    assert_eq!(kept.len(), 2);
    assert!(kept[0].contains("WARN"));
    assert!(kept[1].contains("\"finished\""));
}

#[test]
fn sink_appends_and_rotates() {
    let path = scratch("rotate");
    let mut sink = LogSink::new(path.clone(), 200);
    for i in 0..10 {
        sink.append("install 440", &format!("line {i}"));
    }
    assert!(sink.backup_path().exists());
    assert!(fs::metadata(&path).unwrap().len() <= 200);
    let lines = aurelia_log::read_lines_from(&path);
    assert!(lines.last().unwrap().ends_with("[install 440] line 9"));
    assert!(lines.len() < 10);
}

#[test]
fn label_stops_at_flags() {
    assert_eq!(
        aurelia_log::label(&["install", "440", "--json"]),
        "install 440"
    );
    assert_eq!(
        aurelia_log::label(&["login", "--web-token", "secret"]),
        "login"
    );
    assert_eq!(
        aurelia_log::label(&["config", "proxy", "http://u:p@h"]),
        "config proxy"
    );
}

#[test]
fn timestamp_formats_utc() {
    assert_eq!(aurelia_log::timestamp(0), "1970-01-01 00:00:00");
    assert_eq!(aurelia_log::timestamp(1_795_000_000), "2026-11-18 11:06:40");
}

#[test]
fn log_view_scrolls() {
    let mut view = LogView {
        open: true,
        lines: (0..50).map(|i| i.to_string()).collect(),
        from_bottom: 0,
    };
    assert_eq!(view.visible(5).last().unwrap(), "49");
    view.up(10);
    assert_eq!(view.visible(5).last().unwrap(), "39");
    view.up(1000);
    assert_eq!(view.visible(5), ["0".to_string()]);
    view.down(1000);
    assert_eq!(view.from_bottom, 0);
    view.handle_key(crossterm::event::KeyCode::Esc);
    assert!(!view.open);
}
