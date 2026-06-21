use crate::util::error::STError;
use crate::util::paths::config_location;

use crate::interface::game::GameType;

use serde::{Deserialize, Serialize};
use std::fs;
use std::sync::RwLock;

use tui::style::Color;

/// Process-wide cache of the parsed config. `Config::new`/`save` keep it in sync.
/// Avoids a `config.json` disk read + JSON parse on every `get_name`/`is_valid`
/// call (which the list filter runs for every game, several times per keypress).
static CACHE: RwLock<Option<Config>> = RwLock::new(None);

#[derive(Serialize, Deserialize, Clone)]
pub struct Config {
    pub default_user: String,
    pub hidden_games: Vec<i32>,
    pub favorite_games: Vec<i32>,
    pub allowed_games: Vec<GameType>,
    pub highlight: Color,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            default_user: "".to_string(),
            hidden_games: vec![],
            favorite_games: vec![],
            allowed_games: vec![GameType::Game, GameType::DLC],
            highlight: Color::Green,
        }
    }
}

impl Config {
    pub fn new() -> Result<Config, STError> {
        let config = match serde_json::from_str(&fs::read_to_string(config_location()?)?) {
            Ok(config) => config,
            _ => {
                let config = Config::default();
                config.save()?;
                config
            }
        };
        *CACHE.write()? = Some(config.clone());
        Ok(config)
    }

    /// Cheap, cached read of the config. Loads from disk only on the first call;
    /// thereafter serves a clone of the in-memory copy. Hot path for the list
    /// rendering/filtering, so it must never touch the filesystem on repeat.
    pub fn cached() -> Config {
        {
            let guard = CACHE.read().expect("config cache poisoned");
            if let Some(config) = &*guard {
                return config.clone();
            }
        }
        Config::new().unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), STError> {
        *CACHE.write()? = Some(self.clone());
        fs::write(config_location()?, serde_json::to_string(&self)?)?;
        Ok(())
    }
}
