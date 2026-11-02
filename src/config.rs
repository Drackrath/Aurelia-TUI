use crate::util::error::STError;
use crate::util::paths::config_location;

use crate::interface::game::GameType;

use serde::{Deserialize, Serialize};
use std::fs;
use std::sync::RwLock;

use tui::style::Color;

use crate::theme;

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
    /// TUI-owned metadata preference; unlike runtime plugins, HLTB does not run
    /// through the launcher. Missing fields in older configs preserve the
    /// existing enabled behavior without resetting favourites or hidden games.
    #[serde(default = "default_hltb_enabled")]
    pub hltb_enabled: bool,
}

fn default_hltb_enabled() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Config {
            default_user: "".to_string(),
            hidden_games: vec![],
            favorite_games: vec![],
            allowed_games: vec![GameType::Game, GameType::DLC],
            highlight: theme::ACCENT,
            hltb_enabled: default_hltb_enabled(),
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
        // Publish only persisted settings. A failed save must not make a toggle
        // appear successful in cached readers while the UI retains its old copy.
        fs::write(config_location()?, serde_json::to_string(&self)?)?;
        *CACHE.write()? = Some(self.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_configs_enable_hltb_without_resetting_existing_preferences() {
        let mut legacy = serde_json::to_value(Config::default()).unwrap();
        legacy.as_object_mut().unwrap().remove("hltb_enabled");
        legacy["favorite_games"] = serde_json::json!([400]);
        legacy["hidden_games"] = serde_json::json!([620]);
        legacy["default_user"] = serde_json::json!("saved user");
        let config: Config = serde_json::from_value(legacy).unwrap();
        assert!(config.hltb_enabled);
        assert_eq!(config.favorite_games, [400]);
        assert_eq!(config.hidden_games, [620]);
        assert_eq!(config.default_user, "saved user");
        let disabled = Config {
            hltb_enabled: false,
            ..config
        };
        let restored: Config =
            serde_json::from_str(&serde_json::to_string(&disabled).unwrap()).unwrap();
        assert!(!restored.hltb_enabled);
        assert_eq!(restored.favorite_games, [400]);
    }
}
