use std::thread;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::sync::Mutex;

use crate::interface::aurelia::{self, InfoJson, LibraryGameJson};
use crate::interface::game_status::GameStatus;
use crate::interface::proton_data;
use crate::util::{error::STError, stateful::Named};

use crate::config::Config;

use serde::{Deserialize, Serialize};

#[derive(PartialEq, Debug, Serialize, Deserialize, Clone)]
pub enum GameType {
    Game,
    DLC,
    Driver,

    // Other types, default hidden
    Application,
    Config,
    Demo,
    Tool,
    Unknown,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Game {
    pub id: i32,
    pub name: String,
    pub developer: String,
    pub homepage: String,
    pub publisher: String,
    pub game_type: GameType,
    pub icon_url: Option<String>,

    // Install state, sourced from `aurelia list`. Serialized into the games
    // cache so the install status survives a cache round-trip (the live
    // `status` cell below is not serialized).
    #[serde(default)]
    pub installed: bool,
    #[serde(default)]
    pub install_dir: String,
    #[serde(default)]
    pub update_available: bool,
    #[serde(default)]
    pub size: f64,

    #[serde(skip)]
    proton_tier: Arc<Mutex<Option<String>>>,
    // Lazily-fetched store metadata (developer/publisher/description) from
    // `aurelia info`. `info_requested` guards against re-spawning the fetch.
    #[serde(skip)]
    info: Arc<Mutex<Option<InfoJson>>>,
    #[serde(skip)]
    info_requested: Arc<AtomicBool>,
    #[serde(skip)]
    status: Arc<Mutex<Option<GameStatus>>>,
}

impl Game {
    /// Build a `Game` from one `aurelia list --json` entry. Developer/publisher
    /// aren't in the library listing, so they start as `-` and are filled in
    /// lazily on selection via [`Game::query_info`].
    pub fn from_library(entry: LibraryGameJson) -> Game {
        let icon_url = entry.assets.as_ref().and_then(|a| {
            a.header
                .clone()
                .or_else(|| a.capsule.clone())
                .or_else(|| a.hero.clone())
        });
        Game {
            id: entry.app_id as i32,
            name: entry.name,
            developer: "-".to_string(),
            homepage: entry.store_url.unwrap_or_default(),
            publisher: "-".to_string(),
            // The library listing doesn't carry an app type; default to `Game`
            // so entries aren't hidden by the `allowed_games` filter.
            game_type: GameType::Game,
            icon_url,
            installed: entry.is_installed,
            install_dir: entry.install_path.unwrap_or_default(),
            update_available: entry.update_available,
            size: 0.0,
            proton_tier: Arc::new(Mutex::new(None)),
            info: Arc::new(Mutex::new(None)),
            info_requested: Arc::new(AtomicBool::new(false)),
            status: Arc::new(Mutex::new(None)),
        }
    }

    pub fn query_proton(&self) {
        let guard = {
            let mut tier = self.proton_tier.lock().unwrap();
            if None == *tier {
                *tier = Some("-".to_string());
                true
            } else {
                false
            }
        };
        if guard {
            let reference = self.proton_tier.clone();
            let id = self.id;
            thread::spawn(move || {
                if let Some(response) = proton_data::ProtonData::get(id) {
                    let mut status = reference.lock().unwrap();
                    *status = Some(response.format());
                }
            });
        }
    }

    pub fn get_proton(&self) -> String {
        let status = self.proton_tier.lock().unwrap();
        (*status).clone().unwrap_or_else(|| "-".to_string())
    }

    /// Lazily fetch store metadata (developer/publisher/description) the first
    /// time a game is inspected. Mirrors [`Game::query_proton`]'s fire-and-forget
    /// pattern so the UI never blocks on the `aurelia info` subprocess.
    pub fn query_info(&self) {
        if self.info_requested.swap(true, Ordering::SeqCst) {
            return;
        }
        let reference = self.info.clone();
        let id = self.id;
        thread::spawn(move || {
            if let Ok(info) = aurelia::fetch_info(id) {
                let mut slot = reference.lock().unwrap();
                *slot = Some(info);
            }
        });
    }

    pub fn get_developer(&self) -> String {
        if let Some(info) = &*self.info.lock().unwrap() {
            if !info.developers.is_empty() {
                return info.developers.join(", ");
            }
        }
        self.developer.clone()
    }

    pub fn get_publisher(&self) -> String {
        if let Some(info) = &*self.info.lock().unwrap() {
            if !info.publishers.is_empty() {
                return info.publishers.join(", ");
            }
        }
        self.publisher.clone()
    }

    pub fn get_description(&self) -> String {
        if let Some(info) = &*self.info.lock().unwrap() {
            return info.description.clone();
        }
        String::new()
    }

    /// Whether this game is marked as a favourite in the config.
    pub fn is_favourite(&self) -> bool {
        Config::cached().favorite_games.contains(&self.id)
    }

    /// The raw display name (no favourite marker), for sorting.
    pub fn raw_name(&self) -> &str {
        &self.name
    }

    /// Current install/run status. Prefers a live status set by an in-flight
    /// install/launch; otherwise derives one from the cached install fields so
    /// the listing reflects installed-vs-uninstalled after a cache reload.
    pub fn get_status(&self) -> Option<GameStatus> {
        if let Some(live) = &*self.status.lock().unwrap() {
            return Some(live.clone());
        }
        let state = if !self.installed {
            "uninstalled"
        } else if self.update_available {
            "update available"
        } else {
            "installed"
        };
        Some(GameStatus {
            state: state.to_string(),
            installdir: self.install_dir.clone(),
            size: self.size,
        })
    }

    pub fn update_status(self, new_status: GameStatus) {
        let mut status = self.status.lock().unwrap();
        *status = Some(new_status);
    }

    pub fn move_with_status(game: Game, maybe_status: Option<GameStatus>) -> Game {
        Game {
            status: Arc::new(Mutex::new(maybe_status)),
            ..game
        }
    }

    pub fn status_counter(&self) -> Arc<Mutex<Option<GameStatus>>> {
        self.status.clone()
    }
}

/// Fetch the full library via `aurelia list` and map it into `Game`s.
pub fn load_library() -> Result<Vec<Game>, STError> {
    let entries = aurelia::fetch_library()?;
    Ok(entries.into_iter().map(Game::from_library).collect())
}

impl Named for Game {
    fn get_name(&self) -> String {
        let config = Config::cached();
        if config.favorite_games.contains(&self.id) {
            format!("♡ {}", self.name.clone())
        } else {
            self.name.clone()
        }
    }

    fn is_valid(&self) -> bool {
        let config = Config::cached();
        !&config.hidden_games.contains(&self.id) && config.allowed_games.contains(&self.game_type)
    }
}
