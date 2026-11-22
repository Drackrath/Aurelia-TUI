//! Extra library and store fields.
//!
//! `list --json`: see Aurelia `LibraryGame`.
//! `info --extended`: only when expanded.
//! SteamDB links open off-thread.

use std::collections::HashMap;
use std::sync::Mutex;
use std::thread;

use serde::{Deserialize, Serialize};

/// Extra `list --json` fields, flattened.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryInfoJson {
    /// `None` for unowned local installs.
    #[serde(default)]
    pub playtime_forever_minutes: Option<u32>,
    #[serde(default)]
    pub update_queued: bool,
    /// Only resolved by `list --online`.
    #[serde(default)]
    pub online_required: Option<bool>,
    /// Installed bytes (`SizeOnDisk`).
    #[serde(default)]
    pub size_on_disk: Option<u64>,
}

impl LibraryInfoJson {
    /// Whether the game needs a connection.
    pub fn online_only(&self) -> bool {
        self.online_required == Some(true)
    }

    /// Installed size, if Aurelia reported one.
    pub fn installed_size(&self) -> Option<u64> {
        self.size_on_disk.filter(|&s| s > 0)
    }

    /// Sort key: unknown playtime sorts last.
    pub fn playtime_key(&self) -> i64 {
        self.playtime_forever_minutes.map(i64::from).unwrap_or(-1)
    }
}

/// Human playtime, e.g. `12h 05m`.
pub fn format_playtime(minutes: Option<u32>) -> String {
    match minutes {
        None => "—".to_string(),
        Some(0) => "never played".to_string(),
        Some(m) if m < 60 => format!("{m}m"),
        Some(m) => format!("{}h {:02}m", m / 60, m % 60),
    }
}

/// System requirement lines (`extended.requirements`).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RequirementsJson {
    #[serde(default)]
    pub minimum: Vec<String>,
    #[serde(default)]
    pub recommended: Vec<String>,
}

/// `info --extended` object.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ExtendedInfoJson {
    #[serde(default)]
    pub genres: Vec<String>,
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub metacritic: Option<i64>,
    #[serde(default)]
    pub metacritic_url: Option<String>,
    #[serde(default)]
    pub website: Option<String>,
    #[serde(default)]
    pub requirements: RequirementsJson,
}

/// Session cache key: app and language.
type ExtendedKey = (i32, String);

/// `None` value: requested or failed.
static EXTENDED: Mutex<Option<HashMap<ExtendedKey, Option<ExtendedInfoJson>>>> = Mutex::new(None);

fn extended_key(app_id: i32, lang: Option<&str>) -> ExtendedKey {
    (app_id, lang.unwrap_or_default().to_string())
}

/// Reserve a fetch; false if seen.
pub fn claim_extended(app_id: i32, lang: Option<&str>) -> bool {
    let Ok(mut guard) = EXTENDED.lock() else {
        return false;
    };
    let map = guard.get_or_insert_with(HashMap::new);
    let key = extended_key(app_id, lang);
    if map.contains_key(&key) {
        return false;
    }
    map.insert(key, None);
    true
}

/// Record a finished extended fetch.
pub fn store_extended(app_id: i32, lang: Option<&str>, ext: Option<ExtendedInfoJson>) {
    if let Ok(mut guard) = EXTENDED.lock() {
        guard
            .get_or_insert_with(HashMap::new)
            .insert(extended_key(app_id, lang), ext);
    }
}

/// Cached extended info, if fetched.
pub fn cached_extended(app_id: i32, lang: Option<&str>) -> Option<ExtendedInfoJson> {
    EXTENDED
        .lock()
        .ok()?
        .as_ref()?
        .get(&extended_key(app_id, lang))
        .cloned()
        .flatten()
}

/// Arguments for `aurelia info` (before `--json`).
pub fn info_args<'a>(id: &'a str, lang: Option<&'a str>, extended: bool) -> Vec<&'a str> {
    let mut args = vec!["info", id];
    if extended {
        args.push("--extended");
    }
    if let Some(lang) = lang.filter(|l| !l.is_empty()) {
        args.push("-l");
        args.push(lang);
    }
    args
}

/// Steam API language names Aurelia accepts.
///
/// Mirrors `steam_language` in Aurelia's `src/core/locale.rs`.
pub const LANGUAGES: &[&str] = &[
    "english",
    "german",
    "french",
    "italian",
    "spanish",
    "latam",
    "brazilian",
    "portuguese",
    "schinese",
    "tchinese",
    "japanese",
    "koreana",
    "russian",
    "polish",
    "dutch",
    "swedish",
    "danish",
    "finnish",
    "norwegian",
    "czech",
    "hungarian",
    "romanian",
    "bulgarian",
    "greek",
    "turkish",
    "ukrainian",
    "thai",
    "vietnamese",
    "indonesian",
];

/// Next language; `None` means Aurelia's default.
pub fn next_language(current: Option<&str>) -> Option<String> {
    match current.and_then(|c| LANGUAGES.iter().position(|l| *l == c)) {
        None if current.is_some() => None,
        None => Some(LANGUAGES[0].to_string()),
        Some(i) if i + 1 < LANGUAGES.len() => Some(LANGUAGES[i + 1].to_string()),
        Some(_) => None,
    }
}

/// Settings label for the language row.
pub fn language_label(current: Option<&str>) -> String {
    match current {
        Some(l) if !l.is_empty() => l.to_string(),
        _ => "default (Aurelia config)".to_string(),
    }
}

/// SteamDB app page.
pub fn steamdb_app_url(app_id: i32) -> String {
    format!("https://steamdb.info/app/{app_id}/")
}

/// SteamDB depot manifest history.
pub fn steamdb_depot_manifests_url(depot_id: u32) -> String {
    format!("https://steamdb.info/depot/{depot_id}/manifests/")
}

/// Last browser-open failure.
static OPEN_ERROR: Mutex<Option<String>> = Mutex::new(None);

/// Open `url` on a worker thread.
pub fn open_url(url: String) {
    thread::spawn(move || {
        if let Err(e) = crate::util::browser::open(&url)
            && let Ok(mut slot) = OPEN_ERROR.lock()
        {
            *slot = Some(format!("Open {url}: {e}"));
        }
    });
}

/// Take a pending browser-open error.
pub fn take_open_error() -> Option<String> {
    OPEN_ERROR.lock().ok().and_then(|mut slot| slot.take())
}
