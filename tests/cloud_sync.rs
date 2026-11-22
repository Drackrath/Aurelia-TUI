//! Cloud sync parsing, conflicts, badges, switches.

use std::cell::RefCell;
use std::thread;
use std::time::Duration;

use aurelia_tui::config::Config;
use aurelia_tui::interface::aurelia::CloudDirection;
use aurelia_tui::interface::cloud_play::{AutoSync, decide, launch_failed};
use aurelia_tui::interface::cloud_status::{CloudBadge, CloudStatusStore, ago};
use aurelia_tui::interface::cloud_sync::{
    self, CloudSyncConfig, CloudSyncResult, ConflictPrompt, GameCloudMode, GameCloudSync, Resolve,
    SyncStatus, parse_sync, sync_args, sync_resolving,
};
use aurelia_tui::ui::cloud_sync::{conflict, describe};
use tui::Terminal;
use tui::backend::TestBackend;

fn fixture(name: &str) -> CloudSyncResult {
    let path = format!("{}/tests/fixtures/cloud/{name}", env!("CARGO_MANIFEST_DIR"));
    let value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    parse_sync(value).unwrap()
}

fn json(name: &str) -> String {
    let path = format!("{}/tests/fixtures/cloud/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn parses_every_sync_status() {
    let ok = fixture("sync_ok.json");
    assert_eq!(ok.status, Some(SyncStatus::Ok));
    assert_eq!(ok.outcome(), SyncStatus::Ok);
    assert_eq!(ok.downloaded.len(), 2);
    assert_eq!(ok.summary(), "synced: 2 down, 0 up");
    assert!(ok.details().is_empty());

    let conflicts = fixture("sync_conflicts.json");
    assert_eq!(conflicts.outcome(), SyncStatus::Conflicts);
    assert!(conflicts.needs_resolution());
    assert_eq!(conflicts.conflicts[0].cloud_size, 1024);
    assert_eq!(conflicts.conflicts[0].local_timestamp, 1763800000);
    assert_eq!(conflicts.direction, "up");

    let failed = fixture("sync_failed.json");
    assert_eq!(failed.outcome(), SyncStatus::Failed);
    assert_eq!(
        failed.summary(),
        "sync failed: 1 down, 0 up, 1 failed, 1 skipped"
    );
    let details = failed.details();
    assert!(details[0].starts_with("1 file(s) failed"));
    assert!(details[1].contains("[down] b.sav: HTTP 503"));
    assert!(details[2].contains("%LinuxXdgDataHome%"));
    assert_eq!(details.last().unwrap(), "Save prefix: /pfx");

    let incomplete = fixture("sync_incomplete.json");
    assert_eq!(incomplete.outcome(), SyncStatus::Incomplete);
    assert_eq!(
        incomplete.skipped[0].root_token.as_deref(),
        Some("WinAppDataLocal")
    );
}

#[test]
fn counts_override_a_missing_status() {
    let r: CloudSyncResult = parse_sync(
        serde_json::json!({"failed": [{"filename": "x", "direction": "up", "error": "e"}]}),
    )
    .unwrap();
    assert_eq!(r.outcome(), SyncStatus::Failed);
    let odd: CloudSyncResult = parse_sync(serde_json::json!({"status": "weird"})).unwrap();
    assert_eq!(odd.status, Some(SyncStatus::Unknown));
    assert_eq!(odd.outcome(), SyncStatus::Ok);
}

#[test]
fn resolve_keeps_the_requested_direction() {
    assert_eq!(
        sync_args(620, CloudDirection::Both, None),
        ["cloud", "sync", "620"]
    );
    assert_eq!(
        sync_args(620, CloudDirection::Up, Some(Resolve::Local)),
        ["cloud", "sync", "620", "--up", "--resolve", "local"]
    );
    assert_eq!(
        sync_args(620, CloudDirection::Down, Some(Resolve::Cloud)),
        ["cloud", "sync", "620", "--down", "--resolve", "cloud"]
    );
}

#[test]
fn sync_resolving_reruns_only_on_a_choice() {
    let calls = RefCell::new(Vec::new());
    let run = |r: Option<Resolve>| {
        calls.borrow_mut().push(r);
        Ok(if r.is_none() {
            fixture("sync_conflicts.json")
        } else {
            fixture("sync_ok.json")
        })
    };
    let out = sync_resolving(run, |first| {
        assert_eq!(first.conflicts.len(), 1);
        Some(Resolve::Cloud)
    })
    .unwrap();
    assert_eq!(out.outcome(), SyncStatus::Ok);
    assert_eq!(*calls.borrow(), [None, Some(Resolve::Cloud)]);

    calls.borrow_mut().clear();
    let run = |r: Option<Resolve>| {
        calls.borrow_mut().push(r);
        Ok(fixture("sync_conflicts.json"))
    };
    let left = sync_resolving(run, |_| None).unwrap();
    assert_eq!(left.outcome(), SyncStatus::Conflicts);
    assert_eq!(*calls.borrow(), [None]);

    // No prompt without conflicts.
    let out = sync_resolving(|_| Ok(fixture("sync_ok.json")), |_| panic!("asked")).unwrap();
    assert_eq!(out.outcome(), SyncStatus::Ok);
}

#[test]
fn prompt_slot_serves_workers_and_ui() {
    let prompt = ConflictPrompt {
        app_id: 620,
        direction: CloudDirection::Down,
        conflicts: fixture("sync_conflicts.json").conflicts,
    };
    // Worker blocks until answered.
    let p = prompt.clone();
    let worker = thread::spawn(move || cloud_sync::ask(p));
    while !cloud_sync::prompt_pending() {
        thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(cloud_sync::prompt_view().unwrap(), prompt);
    assert_eq!(cloud_sync::answer(Some(Resolve::Local)), None);
    assert_eq!(worker.join().unwrap(), Some(Resolve::Local));
    assert!(!cloud_sync::prompt_pending());

    // UI prompts come back to caller.
    cloud_sync::post_prompt(prompt.clone());
    assert_eq!(cloud_sync::answer(None), Some(prompt));
    assert!(!cloud_sync::prompt_pending());
}

#[test]
fn status_store_tracks_outcomes_and_last_success() {
    let mut store = CloudStatusStore::default();
    store.mark_syncing(620, 100);
    assert_eq!(store.get(620).unwrap().state, CloudBadge::Syncing);

    let ok = store.record_result(620, &fixture("sync_ok.json"), 200);
    assert_eq!(ok.state, CloudBadge::Ok);
    assert_eq!(ok.last_success, Some(200));

    store.mark_syncing(620, 250);
    assert_eq!(store.get(620).unwrap().last_success, Some(200));
    let c = store.record_result(620, &fixture("sync_conflicts.json"), 300);
    assert_eq!(
        (c.state, c.last_success, c.conflicts),
        (CloudBadge::Conflict, Some(200), 1)
    );

    let f = store.record_result(620, &fixture("sync_failed.json"), 400);
    assert_eq!((f.state, f.failed, f.skipped), (CloudBadge::Error, 1, 1));
    assert!(f.message.unwrap().contains("HTTP 503"));

    let i = store.record_result(621, &fixture("sync_incomplete.json"), 400);
    assert_eq!((i.state, i.last_success), (CloudBadge::Error, None));

    let e = store.record_error(620, "offline", 500);
    assert_eq!((e.state, e.last_success), (CloudBadge::Error, Some(200)));

    // Round trip; stale syncing recovers.
    store.mark_syncing(622, 600);
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("cloud-status-test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cloud_sync_status.json");
    store.save(&path).unwrap();
    let loaded = CloudStatusStore::load(&path);
    assert_eq!(loaded.get(620), store.get(620));
    let stale = loaded.get(622).unwrap();
    assert_eq!(stale.state, CloudBadge::Error);
    assert_eq!(stale.message.as_deref(), Some("sync interrupted"));
    assert!(
        CloudStatusStore::load(&dir.join("missing.json"))
            .records
            .is_empty()
    );
}

#[test]
fn badge_text_and_ages() {
    assert_eq!(ago(1000, 990), "just now");
    assert_eq!(ago(1000, 400), "10m ago");
    assert_eq!(ago(10_000, 2_800), "2h ago");
    assert_eq!(ago(200_000, 0), "2d ago");

    let mut store = CloudStatusStore::default();
    let ok = store.record_result(1, &fixture("sync_ok.json"), 1000);
    assert_eq!(describe(&ok, 1000 + 120), "synced 2m ago");
    let c = store.record_result(1, &fixture("sync_conflicts.json"), 2000);
    assert!(describe(&c, 2000).starts_with("1 conflict(s), open [C]"));
    let e = store.record_error(1, "no network\nmore", 3000);
    assert_eq!(
        describe(&e, 3000 + 3600),
        "error: no network (last ok 1h ago)"
    );
}

#[test]
fn auto_sync_follows_both_switches() {
    assert_eq!(decide(false, Some(true)), AutoSync::TuiOff);
    assert_eq!(decide(true, Some(false)), AutoSync::GameOff);
    assert_eq!(decide(true, Some(true)), AutoSync::Run);
    // Unknown switch keeps Aurelia's default.
    assert_eq!(decide(true, None), AutoSync::Run);
}

#[test]
fn failed_launches_skip_upload() {
    assert!(launch_failed(Some("Failed: game is not installed")));
    assert!(launch_failed(Some("Failed to launch")));
    assert!(launch_failed(Some("failed to launch: no such file")));
    assert!(launch_failed(None));
    assert!(!launch_failed(Some("ran (finished)")));
}

#[test]
fn parses_cloud_switches() {
    let global: CloudSyncConfig = serde_json::from_str(&json("config_cloud_sync.json")).unwrap();
    assert!(global.enable_cloud_sync);
    assert_eq!(global.game_overrides.get("730"), Some(&false));

    let off: GameCloudSync = serde_json::from_str(&json("config_game.json")).unwrap();
    assert_eq!(off.mode(), GameCloudMode::Off);
    assert!(!off.cloud_sync_effective);
    assert_eq!(off.label(), "off");
    assert_eq!(off.mode().next(), GameCloudMode::Auto);

    let auto: GameCloudSync = serde_json::from_str(&json("config_game_auto.json")).unwrap();
    assert_eq!(auto.mode(), GameCloudMode::Auto);
    assert_eq!(auto.label(), "auto (on, from global)");
    assert_eq!(auto.mode().next().as_arg(), "on");
}

#[test]
fn auto_cloud_sync_defaults_on_for_old_configs() {
    let mut legacy = serde_json::to_value(Config::default()).unwrap();
    assert_eq!(legacy["auto_cloud_sync"], true);
    legacy.as_object_mut().unwrap().remove("auto_cloud_sync");
    let config: Config = serde_json::from_value(legacy).unwrap();
    assert!(config.auto_cloud_sync);
}

#[test]
fn conflict_prompt_renders_choices() {
    let prompt = ConflictPrompt {
        app_id: 620,
        direction: CloudDirection::Up,
        conflicts: fixture("sync_conflicts.json").conflicts,
    };
    let mut terminal = Terminal::new(TestBackend::new(90, 14)).unwrap();
    terminal
        .draw(|f| f.render_widget(conflict(&prompt, Some("Portal 2"), 1763800060), f.size()))
        .unwrap();
    let buf = terminal.backend().buffer();
    let text: String = (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf.get(x, y).symbol.clone())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("Portal 2"));
    assert!(text.contains("(upload)"));
    assert!(text.contains("slot1.sav"));
    assert!(text.contains("keep cloud"));
    assert!(text.contains("keep local"));
    assert!(text.contains("leave both"));
}
