//! Launch flow tests on fixtures.

use super::*;

const TARGETS: &str = include_str!("../../tests/fixtures/launch/launch_targets.json");
const TARGETS_STEAM: &str = include_str!("../../tests/fixtures/launch/launch_targets_steam.json");
const FINISHED: &str = include_str!("../../tests/fixtures/launch/play_finished.json");
const FORCED: &str = include_str!("../../tests/fixtures/launch/play_steam_forced.json");
const UNCONFIRMED: &str = include_str!("../../tests/fixtures/launch/play_licence_unconfirmed.json");
const IN_WINE: &str = include_str!("../../tests/fixtures/launch/play_in_wine_steam.json");
const DENIED: &str = include_str!("../../tests/fixtures/launch/play_access_denied.json");
const TIMEOUT: &str = include_str!("../../tests/fixtures/launch/play_timeout.json");
const EARLY_TYPED: &str = include_str!("../../tests/fixtures/launch/play_early_exit_typed.json");

const LONG: Duration = Duration::from_secs(600);

fn targets(raw: &str) -> Vec<LaunchTargetJson> {
    parse_launch_targets(serde_json::from_str(raw).unwrap()).unwrap()
}

#[test]
fn launch_targets_fixture_parses() {
    let t = targets(TARGETS);
    assert_eq!(t.len(), 4);
    assert_eq!(t[0].id, "linux");
    assert!(t[0].default && t[0].available);
    assert_eq!(t[1].missing.as_deref(), Some("steam-runtime"));
    assert!(!t[1].available);
    assert_eq!(t[2].steam_mode.as_deref(), Some("host"));
}

#[test]
fn launch_targets_args_add_steam() {
    assert_eq!(launch_targets_args(570, false), ["launch-targets", "570"]);
    assert_eq!(
        launch_targets_args(570, true),
        ["launch-targets", "570", "--steam"]
    );
}

#[test]
fn steam_flag_from_linux_target_flags() {
    let t = targets(TARGETS_STEAM);
    assert!(t[0].play_opts().steam);
    assert!(!targets(TARGETS)[0].play_opts().steam);
    // Integration flag adds `--steam`.
    let opts = play_target_opts(&targets(TARGETS)[0], true);
    assert!(opts.steam);
    assert_eq!(opts.platform.as_deref(), Some("linux"));
}

#[test]
fn resolve_saved_available_target() {
    let t = targets(TARGETS);
    match resolve_target(&t, Some("windows-host")) {
        TargetChoice::Saved(target) => assert_eq!(target.id, "windows-host"),
        other => panic!("{other:?}"),
    }
    assert!(matches!(resolve_target(&t, None), TargetChoice::Automatic));
    assert!(matches!(
        resolve_target(&t, Some("")),
        TargetChoice::Automatic
    ));
}

#[test]
fn resolve_falls_back_when_saved_unavailable() {
    let t = targets(TARGETS);
    for saved in ["windows-runtime", "gone"] {
        match resolve_target(&t, Some(saved)) {
            TargetChoice::Fallback { saved: s, default } => {
                assert_eq!(s, saved);
                assert_eq!(default.unwrap().id, "linux");
            }
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn plain_play_opts_cover_every_case() {
    let t = || Ok(targets(TARGETS));
    // Automatic: plain play, maybe --steam.
    let (opts, notice) = plain_play_opts(true, None, t());
    assert!(opts.steam && opts.platform.is_none() && notice.is_none());

    let (opts, notice) = plain_play_opts(false, Some("windows-host"), t());
    assert_eq!(opts.platform.as_deref(), Some("windows"));
    assert_eq!(opts.steam_mode.as_deref(), Some("host"));
    assert!(notice.is_none());

    let (opts, notice) = plain_play_opts(true, Some("windows-runtime"), t());
    assert!(opts.platform.is_none() && opts.steam);
    let notice = notice.unwrap();
    assert!(
        notice.contains("windows-runtime is unavailable"),
        "{notice}"
    );
    assert!(notice.contains("Linux (native)"), "{notice}");

    let (opts, notice) = plain_play_opts(false, Some("linux"), Err("offline".into()));
    assert!(opts.platform.is_none());
    assert!(notice.unwrap().contains("offline"));
}

#[test]
fn config_round_trips_saved_target_and_steam_flag() {
    let mut config = Config::default();
    set_saved_target(&mut config, 400, Some("windows-host"));
    set_steam_integration(&mut config, 400, true);
    set_steam_integration(&mut config, 400, true);
    let restored: Config = serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
    assert_eq!(saved_target(&restored, 400), Some("windows-host"));
    assert!(steam_integration(&restored, 400));
    assert_eq!(restored.steam_integration, [400]);

    let mut restored = restored;
    set_saved_target(&mut restored, 400, None);
    set_steam_integration(&mut restored, 400, false);
    assert_eq!(saved_target(&restored, 400), None);
    assert!(!steam_integration(&restored, 400));
}

#[test]
fn older_config_without_launch_fields_loads() {
    let mut legacy = serde_json::to_value(Config::default()).unwrap();
    let obj = legacy.as_object_mut().unwrap();
    obj.remove("launch_targets");
    obj.remove("steam_integration");
    let config: Config = serde_json::from_value(legacy).unwrap();
    assert!(config.launch_targets.is_empty());
    assert!(config.steam_integration.is_empty());
}

#[test]
fn finished_play_succeeds() {
    let outcome = parse_play_output(FINISHED, Some(0), LONG);
    assert!(play_succeeded(&outcome));
    assert_eq!(outcome.status_text(), "ran (finished)");
    assert_eq!(outcome.notice(400), None);
    let wine = parse_play_output(IN_WINE, Some(0), LONG);
    assert!(matches!(
        wine,
        PlayOutcome::Finished { runtime: Some(ref r), .. } if r == "in-wine-steam"
    ));
}

#[test]
fn steam_forced_shows_notice() {
    let outcome = parse_play_output(FORCED, Some(0), LONG);
    assert!(outcome.succeeded());
    assert_eq!(outcome.steam_forced(), Some("family-shared"));
    let notice = outcome.notice(400).unwrap();
    assert!(
        notice.contains("Portal") && notice.contains("Steam Family"),
        "{notice}"
    );
    let unconfirmed = parse_play_output(UNCONFIRMED, Some(0), LONG)
        .notice(400)
        .unwrap();
    assert!(unconfirmed.contains("licence could not be confirmed"));
}

#[test]
fn access_denied_is_typed_failure() {
    let outcome = parse_play_output(DENIED, Some(1), LONG);
    assert!(!play_succeeded(&outcome));
    let PlayOutcome::Failed(err) = &outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!(err.effective_kind(), Some("access_denied"));
    let text = outcome.status_text();
    assert!(
        text.starts_with("Failed: access denied: this Steam account"),
        "{text}"
    );
}

#[test]
fn timeout_is_typed_failure() {
    let outcome = parse_play_output(TIMEOUT, Some(TIMEOUT_EXIT_CODE), LONG);
    let text = outcome.status_text();
    assert!(text.starts_with("Failed: the game never started"), "{text}");
    assert!(text.contains("raise --timeout"), "{text}");
    assert!(outcome.notice(380840).unwrap().contains("in-Wine Steam"));
    // Exit 82 alone means timeout.
    let bare = parse_play_output("", Some(TIMEOUT_EXIT_CODE), LONG);
    let PlayOutcome::Failed(err) = &bare else {
        panic!("{bare:?}")
    };
    assert_eq!(err.effective_kind(), Some("timeout"));
}

#[test]
fn early_exit_today_is_not_finished() {
    // v0.3.1: exit 0 and "finished".
    let outcome = parse_play_output(FINISHED, Some(0), Duration::from_secs(4));
    assert!(!play_succeeded(&outcome));
    assert!(matches!(outcome, PlayOutcome::ExitedEarly { secs: 4, .. }));
    let text = outcome.status_text();
    assert!(
        text.starts_with("Failed") && !text.contains("finished"),
        "{text}"
    );
}

#[test]
fn early_exit_after_t259_is_failure() {
    let outcome = parse_play_output(EARLY_TYPED, Some(1), Duration::from_secs(5));
    assert!(!outcome.succeeded());
    assert!(outcome.status_text().contains("exited 2s after start"));
    // Non-zero without JSON.
    let bare = parse_play_output("", Some(1), LONG);
    assert!(
        bare.status_text()
            .starts_with("Failed: aurelia play exited with code 1")
    );
    // Future non-finished status.
    let failed = parse_play_output(r#"{"status":"failed"}"#, Some(0), LONG);
    assert!(!failed.succeeded());
    // Killed by a signal.
    assert!(!parse_play_output(FINISHED, None, LONG).succeeded());
}

#[test]
fn ndjson_tail_is_read() {
    let stdout = format!("{{\"event\":\"progress\"}}\n{}", FINISHED.replace('\n', ""));
    assert!(parse_play_output(&stdout, Some(0), LONG).succeeded());
    // No output, exit 0: finished.
    assert!(parse_play_output("", Some(0), LONG).succeeded());
}

#[test]
fn notices_queue_and_join() {
    while take_notice().is_some() {}
    push_notice("one".into());
    push_notice("two".into());
    assert_eq!(take_notice().as_deref(), Some("one  two"));
    assert_eq!(take_notice(), None);
}

#[test]
fn runtime_install_reports_progress_then_result() {
    let install = RuntimeInstall::default();
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    assert!(install.start_with(move || {
        rx.recv().ok();
        Err("steam_runtime_runner is not set".into())
    }));
    assert!(install.is_running());
    assert!(!install.start_with(|| Ok(())));
    assert!(
        install
            .progress_text()
            .unwrap()
            .contains("Installing the Steam runtime")
    );
    assert_eq!(install.take_finished(), None);
    tx.send(()).unwrap();
    let mut result = None;
    for _ in 0..200 {
        result = install.take_finished();
        if result.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(result, Some(Err("steam_runtime_runner is not set".into())));
    assert!(!install.is_running());
    assert_eq!(install.progress_text(), None);
}

#[test]
fn progress_text_counts_seconds() {
    assert!(runtime_progress_text(Duration::from_secs(42)).ends_with("42s"));
}
