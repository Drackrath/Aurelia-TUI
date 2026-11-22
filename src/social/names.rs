//! Session name cache and gentle resolver.

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use crate::interface::aurelia::FriendJson;
use crate::interface::social::UserProfileJson;

use super::ProfileSlot;

/// Idle time after scrolling before lookups.
pub const NAV_IDLE: Duration = Duration::from_secs(3);
/// Pause between two background lookups.
pub const LOOKUP_GAP: Duration = Duration::from_secs(5);
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
pub fn missing_note(friends: &[FriendJson], steam_here: bool) -> Vec<&'static str> {
    if !names_missing(friends) {
        return Vec::new();
    }
    if steam_here {
        vec![
            "Steam runs here: names hidden",
            "[p] loads one, or close Steam",
        ]
    } else {
        vec!["Steam sent no names yet", "[p] loads one; names fill in"]
    }
}

impl NameResolver {
    /// Store a profile's name and status.
    pub fn learn(&mut self, profile: &UserProfileJson) {
        self.attempted.insert(profile.steam_id);
        let name = profile.persona_name.clone().filter(|n| !n.is_empty());
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
    pub fn apply(&self, friends: &mut [FriendJson]) {
        for f in friends.iter_mut() {
            let Some(learned) = self.cache.get(&f.steam_id) else {
                continue;
            };
            if lacks_name(f) && learned.name.is_some() {
                f.persona_name = learned.name.clone();
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

    /// First visible unnamed, untried friend.
    pub fn next_lookup(&self, friends: &[FriendJson], selected: usize) -> Option<u64> {
        let rows = self.visible_rows.get().max(1);
        let (start, end) = crate::ui::scroll::window(selected, friends.len(), rows);
        friends[start..end]
            .iter()
            .find(|f| {
                lacks_name(f)
                    && !self.attempted.contains(&f.steam_id)
                    && !self.cache.contains_key(&f.steam_id)
            })
            .map(|f| f.steam_id)
    }
}
