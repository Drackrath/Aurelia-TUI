//! Steam restarts, DLC queue, forced pushes.

use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::interface::aurelia;
use crate::util::error::STError;
use crate::util::log::log;

/// Which Steam-guarded relocation to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Relocate {
    Move,
    Relink,
    Import,
}

impl Relocate {
    /// The CLI subcommand name.
    pub fn verb(self) -> &'static str {
        match self {
            Relocate::Move => "move",
            Relocate::Relink => "relink",
            Relocate::Import => "import",
        }
    }
}

/// `move`/`relink`/`import --json` success payload.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RelocateJson {
    #[serde(default)]
    pub app_id: u32,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub library: String,
    /// Absent for `move`.
    #[serde(default)]
    pub install_path: Option<String>,
    #[serde(default)]
    pub steam_restarted: bool,
}

/// `enable|disable <id> --json` success payload.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SetDlcJson {
    #[serde(default)]
    pub app_id: u32,
    #[serde(default)]
    pub base_app_id: u32,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub steam_restarted: bool,
    /// Steam ran, not restarted.
    #[serde(default)]
    pub steam_restart_required: bool,
}

/// `collections pull|push|sync --json` success payload.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CollectionsResultJson {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub namespace_version: u64,
    #[serde(default)]
    pub count: u64,
}

/// One queued DLC toggle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingDlc {
    pub app_id: u32,
    pub enable: bool,
}

/// Untyped `steam_guard_stop` refusal text.
pub fn is_steam_running_error(message: &str) -> bool {
    message.contains("Steam is running") && message.contains("--restart-steam")
}

/// `check_push_allowed` never-pulled refusal.
pub fn is_never_pulled_error(message: &str) -> bool {
    message.contains("never pulled")
}

/// Error payload text, hint appended.
fn error_message(value: &serde_json::Value) -> Option<String> {
    let err = value.get("error").and_then(|e| e.as_str())?;
    match value.get("hint").and_then(|h| h.as_str()) {
        Some(hint) if !hint.is_empty() => Some(format!("{err} — {hint}")),
        _ => Some(err.to_string()),
    }
}

/// Parse `--json` stdout.
pub fn parse_output(stdout: &str, args: &[&str]) -> Result<serde_json::Value, STError> {
    // Errors are the last NDJSON line.
    let line = stdout
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .ok_or_else(|| {
            STError::Problem(format!(
                "aurelia produced no output for `{}`",
                args.join(" ")
            ))
        })?;
    let value: serde_json::Value = match serde_json::from_str(stdout.trim()) {
        Ok(value) => value,
        Err(_) => serde_json::from_str(line)?,
    };
    match error_message(&value) {
        Some(err) => Err(STError::Problem(err)),
        None => Ok(value),
    }
}

/// Run `aurelia <args> --json`.
pub fn run(args: &[&str]) -> Result<serde_json::Value, STError> {
    aurelia::run_json(args)
}

/// Argument list for a relocation.
pub fn relocate_args(kind: Relocate, app_id: i32, library: &str, restart: bool) -> Vec<String> {
    let mut args = vec![
        kind.verb().to_string(),
        app_id.to_string(),
        library.to_string(),
    ];
    if restart {
        args.push("--restart-steam".to_string());
    }
    args
}

/// Run `move|relink|import <id> <library> [--restart-steam]`.
pub fn relocate(
    kind: Relocate,
    app_id: i32,
    library: &str,
    restart: bool,
) -> Result<RelocateJson, STError> {
    let args = relocate_args(kind, app_id, library, restart);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    Ok(serde_json::from_value(run(&refs)?)?)
}

/// Run `enable|disable <id> [--restart-steam]`.
pub fn set_dlc(app_id: u32, enable: bool, restart: bool) -> Result<SetDlcJson, STError> {
    let id = app_id.to_string();
    let mut args = vec![if enable { "enable" } else { "disable" }, id.as_str()];
    if restart {
        args.push("--restart-steam");
    }
    Ok(serde_json::from_value(run(&args)?)?)
}

/// Run `collections push --yes [--force]`.
pub fn collections_push(force: bool) -> Result<CollectionsResultJson, STError> {
    let mut args = vec!["collections", "push", "--yes"];
    if force {
        args.push("--force");
    }
    Ok(serde_json::from_value(run(&args)?)?)
}

// --- Pending DLC queue ---

/// Replace this app's entry.
pub fn queue_change(queue: &mut Vec<PendingDlc>, change: PendingDlc) {
    queue.retain(|c| c.app_id != change.app_id);
    queue.push(change);
}

/// Drop any queued change for `app_id`.
pub fn drop_change(queue: &mut Vec<PendingDlc>, app_id: u32) {
    queue.retain(|c| c.app_id != app_id);
}

/// Record a toggle's outcome in `queue`.
///
/// Queue only restart-required toggles.
pub fn record_toggle(queue: &mut Vec<PendingDlc>, change: PendingDlc, result: &SetDlcJson) {
    if result.steam_restart_required {
        queue_change(queue, change);
    } else {
        drop_change(queue, change.app_id);
    }
}

/// In-memory copy of the on-disk queue.
static QUEUE: Mutex<Option<Vec<PendingDlc>>> = Mutex::new(None);

fn queue_path() -> Option<PathBuf> {
    crate::util::paths::config_directory()
        .ok()
        .map(|dir| dir.join("pending_dlc.json"))
}

fn load_queue() -> Vec<PendingDlc> {
    queue_path()
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_queue(queue: &[PendingDlc]) {
    let Some(path) = queue_path() else { return };
    if queue.is_empty() {
        let _ = fs::remove_file(path);
    } else if let Ok(text) = serde_json::to_string(queue) {
        let _ = fs::write(path, text);
    }
}

/// Edit cached queue, then persist.
fn with_queue<T>(f: impl FnOnce(&mut Vec<PendingDlc>) -> T) -> T {
    let mut guard = QUEUE.lock().unwrap_or_else(|e| e.into_inner());
    let queue = guard.get_or_insert_with(load_queue);
    let out = f(queue);
    save_queue(queue);
    out
}

/// Queued change for `app_id`.
pub fn pending_for(app_id: u32) -> Option<PendingDlc> {
    let mut guard = QUEUE.lock().unwrap_or_else(|e| e.into_inner());
    guard
        .get_or_insert_with(load_queue)
        .iter()
        .find(|c| c.app_id == app_id)
        .copied()
}

/// Toggle DLC; maybe queue it.
///
/// Returns whether the change is pending.
pub fn toggle_dlc(app_id: u32, enable: bool) -> Result<bool, STError> {
    let result = set_dlc(app_id, enable, false)?;
    let change = PendingDlc { app_id, enable };
    with_queue(|q| record_toggle(q, change, &result));
    Ok(result.steam_restart_required)
}

/// Re-apply queued DLC before launch.
///
/// `--restart-steam` only stops a running Steam.
pub fn apply_pending_dlc() {
    let pending = with_queue(|q| q.clone());
    if pending.is_empty() {
        return;
    }
    for change in &pending {
        if let Err(err) = set_dlc(change.app_id, change.enable, true) {
            log!("Pending DLC change failed", change.app_id, err);
        }
    }
    // Drop failures too: no restart loops.
    with_queue(|q| q.retain(|c| !pending.contains(c)));
}
