//! Session name cache and gentle resolver.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::interface::aurelia::FriendJson;
use crate::interface::social::UserProfileJson;

use super::ProfileSlot;

/// Idle time after scrolling before lookups.
pub const NAV_IDLE: Duration = Duration::from_secs(3);
/// Pause between two background lookups.
pub const LOOKUP_GAP: Duration = Duration::from_secs(5);
/// Stored names older than this refresh.
pub const NAME_MAX_AGE: u64 = 7 * 24 * 60 * 60;
/// Name cache file in the cache dir.
pub const STORE_FILE: &str = "friend_names.json";

/// One persisted name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredName {
    pub name: String,
    /// Unix seconds of the lookup.
    pub fetched_at: u64,
}

/// Persisted names keyed by SteamID string.
pub type NameStore = HashMap<String, StoredName>;

/// Current unix time in seconds.
pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Read the store; corrupt or missing is empty.
pub fn load_store(path: &Path) -> NameStore {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Write the store via temp file rename.
pub fn save_store(path: &Path, store: &NameStore) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(store)?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

/// How often to recheck host Steam.
const STEAM_CHECK: Duration = Duration::from_secs(10);

/// What a profile taught us.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Learned {
    pub name: Option<String>,
    pub state: Option<u32>,
}

/// Session names and resolver bookkeeping.
#[derive(Debug, Clone, Default)]
pub struct NameResolver {
    pub cache: HashMap<u64, Learned>,
    pub attempted: HashSet<u64>,
    pub inflight: Option<(u64, ProfileSlot)>,
    pub last_index: usize,
    pub last_nav: Option<Instant>,
    pub last_done: Option<Instant>,
    pub was_friends: bool,
    /// Steam client runs on this machine.
    pub steam_here: bool,
    pub steam_checked: Option<Instant>,
    /// Friend rows the panel last drew.
    pub visible_rows: Cell<usize>,
    /// Persisted names, loaded once.
    pub store: NameStore,
    pub store_loaded: bool,
    /// Store changed since last save.
    pub store_dirty: bool,
    /// Cached name each friend shows.
    pub applied: HashMap<u64, String>,
}

/// Account type bits equal individual.
pub fn is_individual(steam_id: u64) -> bool {
    (steam_id >> 52) & 0xF == 1
}

/// Friend has no usable name.
pub fn lacks_name(f: &FriendJson) -> bool {
    f.persona_name.as_deref().is_none_or(str::is_empty)
}

/// Every friend arrived without a name.
pub fn names_missing(friends: &[FriendJson]) -> bool {
    !friends.is_empty() && friends.iter().all(lacks_name)
}

/// Host Steam check, like Aurelia's `is_steam_running`.
#[cfg(target_os = "linux")]
pub fn host_steam_running() -> bool {
    let live = |pid: &str| {
        std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .map(|stat| !stat.contains(") Z "))
            .unwrap_or(false)
    };
    let pid_file = std::env::var("HOME")
        .map(|h| std::path::PathBuf::from(h).join(".steam/steam.pid"))
        .ok();
    if let Some(pid) = pid_file
        .and_then(|f| std::fs::read_to_string(f).ok())
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
        && live(&pid)
    {
        return true;
    }
    let Ok(entries) = std::fs::read_dir("/proc") else {
        return false;
    };
    entries.flatten().any(|e| {
        let name = e.file_name();
        let Some(pid) = name.to_str() else {
            return false;
        };
        pid.chars().all(|c| c.is_ascii_digit())
            && std::fs::read_to_string(format!("/proc/{pid}/comm"))
                .map(|c| c.trim() == "steam")
                .unwrap_or(false)
            && live(pid)
    })
}

/// Host Steam check (non-Linux: never).
#[cfg(not(target_os = "linux"))]
pub fn host_steam_running() -> bool {
    false
}

/// Panel note for missing names.
pub fn missing_note(friends: &[FriendJson], steam_here: bool) -> Vec<String> {
    let left = friends.iter().filter(|f| lacks_name(f)).count();
    if steam_here && left > 0 {
        vec![
            "Names fill in by themselves".to_string(),
            format!("{left} left · close Steam for all"),
        ]
    } else if names_missing(friends) {
        vec![
            "Steam sent no names yet".to_string(),
            "Names fill in by themselves".to_string(),
        ]
    } else {
        Vec::new()
    }
}

impl NameResolver {
    /// Adopt persisted names into the cache.
    pub fn adopt_store(&mut self, store: NameStore) {
        for (id, stored) in &store {
            let Ok(id) = id.parse::<u64>() else {
                continue;
            };
            if stored.name.is_empty() || self.cache.contains_key(&id) {
                continue;
            }
            self.cache.insert(
                id,
                Learned {
                    name: Some(stored.name.clone()),
                    state: None,
                },
            );
        }
        self.store = store;
        self.store_loaded = true;
    }

    /// Load from `path` once per session.
    pub fn load_once(&mut self, path: &Path) {
        if !self.store_loaded {
            self.adopt_store(load_store(path));
        }
    }

    /// Save to `path` if anything changed.
    pub fn save_if_dirty(&mut self, path: &Path) {
        if self.store_dirty && save_store(path, &self.store).is_ok() {
            self.store_dirty = false;
        }
    }

    /// Stored name missing or over a week old.
    pub fn is_stale(&self, steam_id: u64, now_unix: u64) -> bool {
        self.store
            .get(&steam_id.to_string())
            .is_none_or(|s| now_unix.saturating_sub(s.fetched_at) >= NAME_MAX_AGE)
    }

    /// Store a profile's name and status.
    pub fn learn(&mut self, profile: &UserProfileJson) {
        self.learn_at(profile, unix_now());
    }

    /// Store a profile at `now_unix`.
    pub fn learn_at(&mut self, profile: &UserProfileJson, now_unix: u64) {
        self.attempted.insert(profile.steam_id);
        let name = profile.persona_name.clone().filter(|n| !n.is_empty());
        if let Some(name) = &name {
            let key = profile.steam_id.to_string();
            let changed = self.store.get(&key).map(|s| &s.name) != Some(name);
            if changed || self.is_stale(profile.steam_id, now_unix) {
                self.store.insert(
                    key,
                    StoredName {
                        name: name.clone(),
                        fetched_at: now_unix,
                    },
                );
                self.store_dirty = true;
            }
        }
        if name.is_none() && profile.persona_state.is_none() {
            return;
        }
        self.cache.insert(
            profile.steam_id,
            Learned {
                name,
                state: profile.persona_state,
            },
        );
    }

    /// Fill missing names and states.
    pub fn apply(&mut self, friends: &mut [FriendJson]) {
        for f in friends.iter_mut() {
            let Some(learned) = self.cache.get(&f.steam_id) else {
                continue;
            };
            let ours = lacks_name(f) || self.applied.get(&f.steam_id) == f.persona_name.as_ref();
            if let Some(name) = &learned.name
                && ours
                && f.persona_name.as_ref() != Some(name)
            {
                f.persona_name = Some(name.clone());
                self.applied.insert(f.steam_id, name.clone());
            }
            if f.persona_state.is_none() {
                f.persona_state = learned.state;
            }
        }
    }

    /// Record selection moves; restart idle timer.
    pub fn note_nav(&mut self, index: usize, in_friends: bool, now: Instant) {
        if in_friends && (!self.was_friends || index != self.last_index) {
            self.last_nav = Some(now);
        }
        self.last_index = index;
        self.was_friends = in_friends;
    }

    /// Recheck host Steam every few seconds.
    pub fn refresh_steam_here(&mut self, now: Instant) {
        let due = self
            .steam_checked
            .is_none_or(|t| now.duration_since(t) >= STEAM_CHECK);
        if due {
            self.steam_here = host_steam_running();
            self.steam_checked = Some(now);
        }
    }

    /// Timers allow a new lookup.
    pub fn lookup_due(&self, now: Instant) -> bool {
        self.inflight.is_none()
            && self
                .last_nav
                .is_some_and(|t| now.duration_since(t) >= NAV_IDLE)
            && self
                .last_done
                .is_none_or(|t| now.duration_since(t) >= LOOKUP_GAP)
    }

    /// First visible unnamed or stale friend.
    pub fn next_lookup(&self, friends: &[FriendJson], selected: usize) -> Option<u64> {
        self.next_lookup_at(friends, selected, unix_now())
    }

    /// `next_lookup` at a given unix time.
    pub fn next_lookup_at(
        &self,
        friends: &[FriendJson],
        selected: usize,
        now_unix: u64,
    ) -> Option<u64> {
        let rows = self.visible_rows.get().max(1);
        let (start, end) = crate::ui::scroll::window(selected, friends.len(), rows);
        friends[start..end]
            .iter()
            .find(|f| {
                let stale = self.applied.get(&f.steam_id) == f.persona_name.as_ref()
                    && self.is_stale(f.steam_id, now_unix);
                !self.attempted.contains(&f.steam_id) && (lacks_name(f) || stale)
            })
            .map(|f| f.steam_id)
    }
}

/// Store path in the TUI cache dir.
pub fn store_path() -> Option<PathBuf> {
    crate::util::paths::cache_directory()
        .ok()
        .map(|d| d.join(STORE_FILE))
}
