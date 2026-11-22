//! Steam Cloud sync results and switches.

use std::sync::Mutex;
use std::sync::mpsc::{Sender, channel};
use std::thread;
use std::time::Duration;

use serde::Deserialize;

use crate::interface::aurelia::{CloudDirection, run_json};
use crate::util::error::STError;

/// Overall `status` of `cloud sync`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SyncStatus {
    Ok,
    Conflicts,
    Failed,
    Incomplete,
    #[serde(other)]
    Unknown,
}

/// One diverged save file.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct CloudConflict {
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub local_path: String,
    #[serde(default)]
    pub local_size: u64,
    #[serde(default)]
    pub local_timestamp: u64,
    #[serde(default)]
    pub cloud_size: u64,
    #[serde(default)]
    pub cloud_timestamp: u64,
}

/// A file Aurelia could not place.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct CloudSkip {
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub root_token: Option<String>,
    #[serde(default)]
    pub reason: String,
}

/// A transfer that failed.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct CloudFailure {
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub direction: String,
    #[serde(default)]
    pub error: String,
}

/// `aurelia cloud sync <id> --json` result.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct CloudSyncResult {
    #[serde(default)]
    pub app_id: u32,
    #[serde(default)]
    pub direction: String,
    #[serde(default)]
    pub status: Option<SyncStatus>,
    #[serde(default)]
    pub downloaded: Vec<String>,
    #[serde(default)]
    pub uploaded: Vec<String>,
    #[serde(default)]
    pub conflicts: Vec<CloudConflict>,
    #[serde(default)]
    pub skipped: Vec<CloudSkip>,
    #[serde(default)]
    pub skipped_root_tokens: Vec<String>,
    #[serde(default)]
    pub failed: Vec<CloudFailure>,
    #[serde(default)]
    pub remote_root: Option<String>,
    #[serde(default)]
    pub wine_prefix: Option<String>,
}

impl CloudSyncResult {
    /// Status, trusting counts over the label.
    pub fn outcome(&self) -> SyncStatus {
        if !self.failed.is_empty() {
            SyncStatus::Failed
        } else if !self.conflicts.is_empty() {
            SyncStatus::Conflicts
        } else if !self.skipped.is_empty() {
            SyncStatus::Incomplete
        } else {
            match self.status {
                Some(SyncStatus::Unknown) | None => SyncStatus::Ok,
                Some(s) => s,
            }
        }
    }

    /// Whether a conflict prompt is needed.
    pub fn needs_resolution(&self) -> bool {
        !self.conflicts.is_empty()
    }

    /// One-line summary for status lines.
    pub fn summary(&self) -> String {
        let mut parts = vec![format!(
            "{} down, {} up",
            self.downloaded.len(),
            self.uploaded.len()
        )];
        if !self.conflicts.is_empty() {
            parts.push(format!("{} conflict(s)", self.conflicts.len()));
        }
        if !self.failed.is_empty() {
            parts.push(format!("{} failed", self.failed.len()));
        }
        if !self.skipped.is_empty() {
            parts.push(format!("{} skipped", self.skipped.len()));
        }
        let head = match self.outcome() {
            SyncStatus::Ok => "synced",
            SyncStatus::Conflicts => "conflicts",
            SyncStatus::Failed => "sync failed",
            SyncStatus::Incomplete | SyncStatus::Unknown => "incomplete",
        };
        format!("{head}: {}", parts.join(", "))
    }

    /// Failed and skipped details, for overlays.
    pub fn details(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if !self.failed.is_empty() {
            lines.push(format!(
                "{} file(s) failed to transfer — save set incomplete",
                self.failed.len()
            ));
            for f in self.failed.iter().take(3) {
                lines.push(format!("  [{}] {}: {}", f.direction, f.filename, f.error));
            }
        }
        if !self.skipped.is_empty() {
            let mut line = format!("{} file(s) skipped (no place on disk)", self.skipped.len());
            if !self.skipped_root_tokens.is_empty() {
                let tokens: Vec<String> = self
                    .skipped_root_tokens
                    .iter()
                    .map(|t| format!("%{t}%"))
                    .collect();
                line.push_str(&format!(" — unmapped root {}", tokens.join(", ")));
            }
            lines.push(line);
        }
        if !lines.is_empty()
            && let Some(prefix) = self.wine_prefix.as_deref().filter(|p| !p.is_empty())
        {
            lines.push(format!("Save prefix: {prefix}"));
        }
        lines
    }
}

/// Which side wins a conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolve {
    Cloud,
    Local,
}

impl Resolve {
    /// The `--resolve` value.
    pub fn as_arg(self) -> &'static str {
        match self {
            Resolve::Cloud => "cloud",
            Resolve::Local => "local",
        }
    }
}

/// `cloud sync` args, minus `--json`.
pub fn sync_args(app_id: i32, direction: CloudDirection, resolve: Option<Resolve>) -> Vec<String> {
    let mut args = vec!["cloud".to_string(), "sync".to_string(), app_id.to_string()];
    match direction {
        CloudDirection::Both => {}
        CloudDirection::Down => args.push("--down".to_string()),
        CloudDirection::Up => args.push("--up".to_string()),
    }
    if let Some(r) = resolve {
        args.push("--resolve".to_string());
        args.push(r.as_arg().to_string());
    }
    args
}

/// Parse a `cloud sync` JSON value.
pub fn parse_sync(value: serde_json::Value) -> Result<CloudSyncResult, STError> {
    Ok(serde_json::from_value(value)?)
}

/// Run one `cloud sync` (blocking).
pub fn sync(
    app_id: i32,
    direction: CloudDirection,
    resolve: Option<Resolve>,
) -> Result<CloudSyncResult, STError> {
    let args = sync_args(app_id, direction, resolve);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    parse_sync(run_json(&refs)?)
}

/// Sync, asking once on conflicts.
pub fn sync_resolving(
    mut run: impl FnMut(Option<Resolve>) -> Result<CloudSyncResult, STError>,
    ask: impl FnOnce(&CloudSyncResult) -> Option<Resolve>,
) -> Result<CloudSyncResult, STError> {
    let first = run(None)?;
    if !first.needs_resolution() {
        return Ok(first);
    }
    match ask(&first) {
        Some(choice) => run(Some(choice)),
        None => Ok(first),
    }
}

// --- Conflict prompt shared with UI ---

/// A conflict waiting for the user.
#[derive(Debug, Clone, PartialEq)]
pub struct ConflictPrompt {
    pub app_id: i32,
    pub direction: CloudDirection,
    pub conflicts: Vec<CloudConflict>,
}

struct Pending {
    prompt: ConflictPrompt,
    reply: Option<Sender<Option<Resolve>>>,
}

static PROMPT: Mutex<Option<Pending>> = Mutex::new(None);

/// The open prompt, if any.
pub fn prompt_view() -> Option<ConflictPrompt> {
    PROMPT
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|p| p.prompt.clone()))
}

/// Whether a prompt awaits an answer.
pub fn prompt_pending() -> bool {
    PROMPT.lock().map(|g| g.is_some()).unwrap_or(false)
}

/// Show a prompt the UI resolves.
pub fn post_prompt(prompt: ConflictPrompt) {
    if let Ok(mut g) = PROMPT.lock() {
        *g = Some(Pending {
            prompt,
            reply: None,
        });
    }
}

/// Block a worker until answered.
pub fn ask(prompt: ConflictPrompt) -> Option<Resolve> {
    let (tx, rx) = channel();
    let mut pending = Some(Pending {
        prompt,
        reply: Some(tx),
    });
    // Wait for any earlier prompt.
    while pending.is_some() {
        match PROMPT.lock() {
            Ok(mut g) if g.is_none() => *g = pending.take(),
            Ok(_) => thread::sleep(Duration::from_millis(200)),
            Err(_) => return None,
        }
    }
    rx.recv().ok().flatten()
}

/// Answer; returns unowned prompts back.
pub fn answer(choice: Option<Resolve>) -> Option<ConflictPrompt> {
    let pending = PROMPT.lock().ok()?.take()?;
    match pending.reply {
        Some(tx) => {
            let _ = tx.send(choice);
            None
        }
        None => Some(pending.prompt),
    }
}

// --- Global and per-game switches ---

/// `aurelia config cloud-sync --json` output.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct CloudSyncConfig {
    #[serde(default)]
    pub enable_cloud_sync: bool,
    #[serde(default)]
    pub game_overrides: std::collections::BTreeMap<String, bool>,
}

/// Read the global switch.
pub fn global() -> Result<CloudSyncConfig, STError> {
    Ok(serde_json::from_value(run_json(&[
        "config",
        "cloud-sync",
    ])?)?)
}

/// Set the global switch.
pub fn set_global(on: bool) -> Result<CloudSyncConfig, STError> {
    let mode = if on { "on" } else { "off" };
    Ok(serde_json::from_value(run_json(&[
        "config",
        "cloud-sync",
        mode,
    ])?)?)
}

/// Per-game override choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameCloudMode {
    Auto,
    On,
    Off,
}

impl GameCloudMode {
    /// The `--cloud-sync` value.
    pub fn as_arg(self) -> &'static str {
        match self {
            GameCloudMode::Auto => "auto",
            GameCloudMode::On => "on",
            GameCloudMode::Off => "off",
        }
    }

    /// Cycle auto, on, off.
    pub fn next(self) -> GameCloudMode {
        match self {
            GameCloudMode::Auto => GameCloudMode::On,
            GameCloudMode::On => GameCloudMode::Off,
            GameCloudMode::Off => GameCloudMode::Auto,
        }
    }
}

/// Cloud keys of `config game --json`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct GameCloudSync {
    #[serde(default)]
    pub cloud_sync: Option<bool>,
    #[serde(default = "default_true")]
    pub cloud_sync_effective: bool,
}

fn default_true() -> bool {
    true
}

impl GameCloudSync {
    /// The stored override as a mode.
    pub fn mode(&self) -> GameCloudMode {
        match self.cloud_sync {
            None => GameCloudMode::Auto,
            Some(true) => GameCloudMode::On,
            Some(false) => GameCloudMode::Off,
        }
    }

    /// Row label, e.g. "auto (on)".
    pub fn label(&self) -> String {
        let eff = if self.cloud_sync_effective {
            "on"
        } else {
            "off"
        };
        match self.cloud_sync {
            None => format!("auto ({eff}, from global)"),
            Some(_) => eff.to_string(),
        }
    }
}

/// Read a game's cloud switch.
pub fn game(app_id: i32) -> Result<GameCloudSync, STError> {
    Ok(serde_json::from_value(run_json(&[
        "config",
        "game",
        &app_id.to_string(),
    ])?)?)
}

/// Set a game's cloud switch.
pub fn set_game(app_id: i32, mode: GameCloudMode) -> Result<GameCloudSync, STError> {
    Ok(serde_json::from_value(run_json(&[
        "config",
        "game",
        &app_id.to_string(),
        "--cloud-sync",
        mode.as_arg(),
    ])?)?)
}
