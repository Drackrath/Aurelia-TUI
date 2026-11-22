//! Restart, DLC queue, depot files tests.

use aurelia_tui::interface::depot_files::{
    self, DepotDownloadJson, DepotFilesJson, DownloadOutcome, ManifestSel,
};
use aurelia_tui::interface::steam_restart::{
    self, CollectionsResultJson, PendingDlc, Relocate, RelocateJson, SetDlcJson,
};
use aurelia_tui::restart_depots::{DownloadState, FileBrowser, FileInput, files_key};
use aurelia_tui::ui::restart_depots as view;
use crossterm::event::KeyCode;
use tui::Terminal;
use tui::backend::TestBackend;

fn fixture(path: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/fixtures/{path}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap()
}

fn parse(path: &str) -> Result<serde_json::Value, String> {
    steam_restart::parse_output(&fixture(path), &["test"]).map_err(|e| e.to_string())
}

// --- Steam running ---

#[test]
fn steam_running_refusal_is_detected() {
    let err = parse("steam_restart/steam_running_error.json").unwrap_err();
    assert!(steam_restart::is_steam_running_error(&err));
    assert!(!steam_restart::is_steam_running_error(
        "failed to move app 620"
    ));
}

#[test]
fn relocate_args_add_restart_flag() {
    assert_eq!(
        steam_restart::relocate_args(Relocate::Move, 620, "/lib", false),
        ["move", "620", "/lib"]
    );
    assert_eq!(
        steam_restart::relocate_args(Relocate::Import, 620, "/lib", true),
        ["import", "620", "/lib", "--restart-steam"]
    );
    assert_eq!(Relocate::Relink.verb(), "relink");
}

#[test]
fn relocate_payloads_parse() {
    let moved: RelocateJson =
        serde_json::from_value(parse("steam_restart/move.json").unwrap()).unwrap();
    assert_eq!(moved.status, "moved");
    assert!(moved.steam_restarted);
    assert!(moved.install_path.is_none());

    let relinked: RelocateJson =
        serde_json::from_value(parse("steam_restart/relink.json").unwrap()).unwrap();
    assert_eq!(relinked.status, "relinked");
    assert!(relinked.install_path.unwrap().ends_with("Portal 2"));
}

#[test]
fn ndjson_last_line_error_is_found() {
    let stdout = format!(
        "{{\"event\":\"progress\"}}\n{}",
        fixture("steam_restart/steam_running_error.json")
    );
    let err = steam_restart::parse_output(&stdout, &["move"]).unwrap_err();
    assert!(steam_restart::is_steam_running_error(&err.to_string()));
    assert!(steam_restart::parse_output("  ", &["move"]).is_err());
}

// --- DLC queue ---

#[test]
fn dlc_payloads_parse() {
    let pending: SetDlcJson =
        serde_json::from_value(parse("steam_restart/dlc_restart_required.json").unwrap()).unwrap();
    assert_eq!(pending.app_id, 1868140);
    assert_eq!(pending.base_app_id, 1145360);
    assert!(pending.steam_restart_required);
    let done: SetDlcJson =
        serde_json::from_value(parse("steam_restart/dlc_restarted.json").unwrap()).unwrap();
    assert!(done.steam_restarted && !done.steam_restart_required);
}

#[test]
fn only_restart_required_toggles_queue() {
    let mut queue = Vec::new();
    let required = SetDlcJson {
        steam_restart_required: true,
        ..SetDlcJson::default()
    };
    let applied = SetDlcJson::default();

    let off = PendingDlc {
        app_id: 10,
        enable: false,
    };
    steam_restart::record_toggle(&mut queue, off, &required);
    assert_eq!(queue, [off]);

    // Later toggle replaces earlier.
    let on = PendingDlc {
        app_id: 10,
        enable: true,
    };
    steam_restart::record_toggle(&mut queue, on, &required);
    assert_eq!(queue, [on]);

    // Steam closed: the edit stands.
    steam_restart::record_toggle(&mut queue, off, &applied);
    assert!(queue.is_empty());
}

// --- Collections ---

#[test]
fn never_pulled_refusal_is_detected() {
    let err = parse("steam_restart/push_never_pulled.json").unwrap_err();
    assert!(steam_restart::is_never_pulled_error(&err));
    assert!(!steam_restart::is_never_pulled_error("network timeout"));
}

#[test]
fn collections_results_still_parse() {
    for (file, status, count) in [
        ("collections_pull.json", "pulled", 7),
        ("collections_push.json", "pushed", 7),
        ("collections_sync.json", "synced", 8),
    ] {
        let value = parse(&format!("steam_restart/{file}")).unwrap();
        let done: CollectionsResultJson = serde_json::from_value(value).unwrap();
        assert_eq!(done.status, status);
        assert_eq!(done.count, count);
        assert!(done.namespace_version > 0);
    }
}

// --- Depot files ---

fn listing() -> DepotFilesJson {
    serde_json::from_value(parse("depot_files/files.json").unwrap()).unwrap()
}

#[test]
fn files_payload_parses() {
    let l = listing();
    assert_eq!(l.depot_id, 621);
    assert_eq!(l.manifest_id, 7230931924467582153);
    assert_eq!(l.files.len(), 4);
    assert!(l.files[0].is_dir());
    assert!(l.files[1].is_file() && l.files[1].executable);
    assert_eq!(l.files[3].link_target.as_deref(), Some("libfoo.so.1"));
    assert_eq!(view::entry_label(&l.files[0]), "bin/");
    assert_eq!(view::entry_label(&l.files[1]), "bin/linux64/portal2_linux*");
    assert_eq!(
        view::entry_label(&l.files[3]),
        "lib/libfoo.so -> libfoo.so.1"
    );
    assert_eq!(view::entry_size(&l.files[0]), "<dir>");
}

#[test]
fn manifest_selection_maps_to_flags() {
    assert_eq!(ManifestSel::parse(""), ManifestSel::Current);
    assert_eq!(ManifestSel::parse("public"), ManifestSel::Current);
    assert_eq!(ManifestSel::parse("123"), ManifestSel::Manifest(123));
    assert_eq!(
        ManifestSel::parse(" beta "),
        ManifestSel::Branch("beta".into())
    );

    assert_eq!(
        depot_files::files_args(620, 621, &ManifestSel::Current, ""),
        ["depots", "files", "620", "621"]
    );
    assert_eq!(
        depot_files::files_args(620, 621, &ManifestSel::Branch("beta".into()), "*.so"),
        [
            "depots", "files", "620", "621", "--branch", "beta", "--filter", "*.so"
        ]
    );
    assert_eq!(
        depot_files::download_args(620, 621, "a/b.txt", &ManifestSel::Manifest(9), "/d", true),
        [
            "depots",
            "download",
            "620",
            "621",
            "a/b.txt",
            "--manifest",
            "9",
            "--output",
            "/d",
            "--force"
        ]
    );
}

#[test]
fn download_outcomes_classify() {
    let done: DepotDownloadJson =
        serde_json::from_value(parse("depot_files/download.json").unwrap()).unwrap();
    assert_eq!(done.size, 1024);

    let ok = depot_files::outcome("abc", 1024, Ok(done.clone()));
    assert!(matches!(ok, DownloadOutcome::Verified { .. }));
    assert!(ok.line().contains("SHA-1 OK (abc)"));

    let empty = depot_files::outcome("", 0, Ok(done));
    assert!(matches!(empty, DownloadOutcome::Unchecked { .. }));

    let err = |file: &str| {
        steam_restart::parse_output(&fixture(file), &["depots"])
            .map(|_| DepotDownloadJson::default())
    };
    let bad = depot_files::outcome("abc", 1, err("depot_files/checksum_error.json"));
    assert!(matches!(bad, DownloadOutcome::ChecksumFailed(_)));
    assert!(bad.line().starts_with("SHA-1 mismatch"));
    let exists = depot_files::outcome("abc", 1, err("depot_files/exists_error.json"));
    assert!(matches!(exists, DownloadOutcome::Exists(_)));
}

#[test]
fn file_browser_keys_navigate_and_prompt() {
    let mut files = FileBrowser {
        app_id: 620,
        depot_id: 621,
        last_dir: "~/Downloads".into(),
        ..FileBrowser::default()
    };
    files.apply_listing(Ok(listing()));

    // Directories don't download.
    assert!(!files_key(&mut files, KeyCode::Enter));
    assert!(files.input.is_none());

    files_key(&mut files, KeyCode::Char('j'));
    files_key(&mut files, KeyCode::Enter);
    let (field, text) = files.input.clone().unwrap();
    assert_eq!(field, FileInput::Target);
    assert_eq!(text, "~/Downloads");

    // Esc drops the prompt only.
    assert!(!files_key(&mut files, KeyCode::Esc));
    assert!(files.input.is_none());

    files_key(&mut files, KeyCode::Char('/'));
    files_key(&mut files, KeyCode::Char('x'));
    assert_eq!(files.input.clone().unwrap().1, "x");
    files_key(&mut files, KeyCode::Backspace);
    files_key(&mut files, KeyCode::Esc);

    // Overwrite prompt: n clears it.
    *files.download.lock().unwrap() = DownloadState::Done(DownloadOutcome::Exists("exists".into()));
    assert!(files.asks_overwrite());
    files_key(&mut files, KeyCode::Char('n'));
    assert_eq!(files.download_state(), DownloadState::Idle);

    assert!(files_key(&mut files, KeyCode::Esc));
}

#[test]
fn file_browser_renders_listing_and_status() {
    let mut files = FileBrowser {
        depot_label: "Portal 2 Linux (621)".into(),
        ..FileBrowser::default()
    };
    files.apply_listing(Ok(listing()));
    *files.download.lock().unwrap() = DownloadState::Done(DownloadOutcome::Verified {
        path: "/d/portal2.sh".into(),
        sha1: "de9f".into(),
    });

    let mut terminal = Terminal::new(TestBackend::new(90, 16)).unwrap();
    terminal
        .draw(|f| {
            let area = f.size();
            let top = tui::layout::Rect { height: 10, ..area };
            let bottom = tui::layout::Rect {
                y: 10,
                height: 6,
                ..area
            };
            f.render_widget(view::file_list(&files, 8), top);
            f.render_widget(view::file_footer(&files), bottom);
        })
        .unwrap();
    let buf = terminal.backend().buffer();
    let text: String = (0..16)
        .map(|y| {
            (0..90)
                .map(|x| buf.get(x, y).symbol.clone())
                .collect::<String>()
                + "\n"
        })
        .collect();
    assert!(text.contains("portal2.sh"));
    assert!(text.contains("manifest 7230931924467582153"));
    assert!(text.contains("SHA-1 OK"));
    assert!(text.contains("2 files"));
}

#[test]
fn question_box_renders() {
    let mut terminal = Terminal::new(TestBackend::new(60, 8)).unwrap();
    terminal
        .draw(|f| {
            f.render_widget(
                view::question_box(
                    aurelia_tui::restart_depots::RESTART_QUESTION.to_string(),
                    String::new(),
                ),
                f.size(),
            )
        })
        .unwrap();
    let buf = terminal.backend().buffer();
    let row: String = (0..60).map(|x| buf.get(x, 1).symbol.clone()).collect();
    assert!(row.contains("Steam is running: restart Steam"));
}
