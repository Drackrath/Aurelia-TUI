//! Last Cloud sync outcome per game.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::interface::cloud_sync::{CloudSyncResult, SyncStatus};
use crate::util::paths::cache_directory;

/// File name inside the cache dir.
pub const STORE_FILE: &str = "cloud_sync_status.json";

/// Badge state shown per game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CloudBadge {
    Syncing,
    Ok,
    Conflict,
    Error,
}

/// One game's last sync record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CloudRecord {
    pub state: CloudBadge,
    /// Unix seconds of last success.
    #[serde(default)]
    pub last_success: Option<u64>,
    /// Unix seconds of last change.
    #[serde(default)]
    pub updated: u64,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub conflicts: usize,
    #[serde(default)]
    pub failed: usize,
    #[serde(default)]
    pub skipped: usize,
}

/// All records, keyed by app id.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CloudStatusStore {
    pub records: BTreeMap<i32, CloudRecord>,
}

impl CloudStatusStore {
    /// Load; stale "syncing" becomes error.
    pub fn load(path: &Path) -> CloudStatusStore {
        let mut store: CloudStatusStore = fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        for record in store.records.values_mut() {
            if record.state == CloudBadge::Syncing {
                record.state = CloudBadge::Error;
                record.message = Some("sync interrupted".to_string());
            }
        }
        store
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        fs::write(path, serde_json::to_string(self)?)
    }

    pub fn get(&self, app_id: i32) -> Option<&CloudRecord> {
        self.records.get(&app_id)
    }

    /// Flag a sync as in flight.
    pub fn mark_syncing(&mut self, app_id: i32, now: u64) {
        let last_success = self.get(app_id).and_then(|r| r.last_success);
        self.records.insert(
            app_id,
            CloudRecord {
                state: CloudBadge::Syncing,
                last_success,
                updated: now,
                message: None,
                conflicts: 0,
                failed: 0,
                skipped: 0,
            },
        );
    }

    /// Record a finished sync result.
    pub fn record_result(
        &mut self,
        app_id: i32,
        result: &CloudSyncResult,
        now: u64,
    ) -> CloudRecord {
        let previous = self.get(app_id).and_then(|r| r.last_success);
        let (state, last_success) = match result.outcome() {
            SyncStatus::Ok => (CloudBadge::Ok, Some(now)),
            SyncStatus::Conflicts => (CloudBadge::Conflict, previous),
            _ => (CloudBadge::Error, previous),
        };
        let details = result.details();
        let record = CloudRecord {
            state,
            last_success,
            updated: now,
            message: (!details.is_empty()).then(|| details.join("\n")),
            conflicts: result.conflicts.len(),
            failed: result.failed.len(),
            skipped: result.skipped.len(),
        };
        self.records.insert(app_id, record.clone());
        record
    }

    /// Record a sync that errored out.
    pub fn record_error(&mut self, app_id: i32, message: &str, now: u64) -> CloudRecord {
        let last_success = self.get(app_id).and_then(|r| r.last_success);
        let record = CloudRecord {
            state: CloudBadge::Error,
            last_success,
            updated: now,
            message: Some(message.to_string()),
            conflicts: 0,
            failed: 0,
            skipped: 0,
        };
        self.records.insert(app_id, record.clone());
        record
    }
}

/// Current Unix time in seconds.
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Short age like "5m ago".
pub fn ago(now: u64, then: u64) -> String {
    let secs = now.saturating_sub(then);
    if secs < 60 {
        "just now".to_string()
    } else if secs < 3600 {
        format!("{}m ago", secs / 60)
    } else if secs < 86_400 {
        format!("{}h ago", secs / 3600)
    } else {
        format!("{}d ago", secs / 86_400)
    }
}

// --- Process-wide cached store ---

static STORE: Mutex<Option<(PathBuf, CloudStatusStore)>> = Mutex::new(None);

fn with_store<T>(f: impl FnOnce(&mut CloudStatusStore) -> T, persist: bool) -> Option<T> {
    let mut guard = STORE.lock().ok()?;
    if guard.is_none() {
        let path = cache_directory().ok()?.join(STORE_FILE);
        let store = CloudStatusStore::load(&path);
        *guard = Some((path, store));
    }
    let (path, store) = guard.as_mut()?;
    let out = f(store);
    if persist {
        let _ = store.save(path);
    }
    Some(out)
}

/// Last record for a game.
pub fn get(app_id: i32) -> Option<CloudRecord> {
    with_store(|s| s.get(app_id).cloned(), false).flatten()
}

pub fn mark_syncing(app_id: i32) {
    with_store(|s| s.mark_syncing(app_id, now()), true);
}

pub fn record_result(app_id: i32, result: &CloudSyncResult) {
    with_store(|s| s.record_result(app_id, result, now()), true);
}

pub fn record_error(app_id: i32, message: &str) {
    with_store(|s| s.record_error(app_id, message, now()), true);
}
