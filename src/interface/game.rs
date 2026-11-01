use std::thread;

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::interface::aurelia::{self, InfoJson, LibraryGameJson};
use crate::interface::game_status::GameStatus;
use crate::interface::hltb::{self, HltbData, HltbState};
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
    /// Portrait box/cover art (`assets.capsule`, e.g. `library_600x900.jpg`) —
    /// distinct from the wide `icon_url` used as the detail-pane background.
    #[serde(default)]
    pub cover_url: Option<String>,

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
    #[serde(skip)]
    hltb: Arc<Mutex<HltbState>>,
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
        // The cover art proper is the portrait library capsule (600x900).
        let cover_url = entry
            .assets
            .as_ref()
            .and_then(|a| a.capsule.clone().or_else(|| a.header.clone()));
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
            cover_url,
            installed: entry.is_installed,
            install_dir: entry.install_path.unwrap_or_default(),
            update_available: entry.update_available,
            size: 0.0,
            proton_tier: Arc::new(Mutex::new(None)),
            hltb: Arc::new(Mutex::new(HltbState::Pending)),
            info: Arc::new(Mutex::new(None)),
            info_requested: Arc::new(AtomicBool::new(false)),
            status: Arc::new(Mutex::new(None)),
        }
    }

    pub fn query_proton(&self) {
        let guard = match self.proton_tier.lock() {
            Ok(mut tier) => {
                if None == *tier {
                    *tier = Some("-".to_string());
                    true
                } else {
                    false
                }
            }
            Err(_) => false,
        };
        if guard {
            let reference = self.proton_tier.clone();
            let id = self.id;
            thread::spawn(move || {
                if let Some(response) = proton_data::ProtonData::get(id) {
                    if let Ok(mut status) = reference.lock() {
                        *status = Some(response.format());
                    }
                }
            });
        }
    }

    pub fn get_proton(&self) -> String {
        self.proton_tier
            .lock()
            .ok()
            .and_then(|status| status.clone())
            .unwrap_or_else(|| "-".to_string())
    }

    /// Start one lookup after selection settles, like the store-info/Proton
    /// paths. Cloned games share this slot, preventing duplicate work. If the
    /// service is busy, leave Pending so only a still-selected game retries on
    /// a later idle tick; do not queue every game the user scrolled past.
    pub fn query_hltb(&self) {
        self.query_hltb_with(hltb::lookup);
    }

    fn query_hltb_with(
        &self,
        lookup: impl FnOnce(i32, &str, &mut dyn FnMut(HltbData)) -> HltbState + Send + 'static,
    ) {
        // Guard here as well as in the idle dispatcher so callers cannot start
        // HLTB work while disabled, even when a cached result already exists.
        if !Config::cached().hltb_enabled {
            return;
        }
        let Ok(mut state) = self.hltb.lock() else {
            return;
        };
        if !matches!(*state, HltbState::Pending) {
            return;
        }
        let Some(permit) = hltb::reserve() else {
            return;
        };
        *state = HltbState::Loading;
        let reference = self.hltb.clone();
        let id = self.id;
        let name = self.name.clone();
        thread::spawn(move || {
            let _permit = permit;
            // A toggle can change after scheduling but before this thread runs.
            // Leave the game retryable if it was disabled in that interval.
            let result = if Config::cached().hltb_enabled {
                lookup(id, &name, &mut |data| {
                    if let Ok(mut state) = reference.lock() {
                        *state = HltbState::Ready { data, stale: true };
                    }
                })
            } else {
                HltbState::Pending
            };
            if let Ok(mut state) = reference.lock() {
                *state = result;
            }
        });
    }

    /// Retry only failures, without reloading the Steam library. Leave scheduling
    /// to the idle dispatcher so repeated input cannot queue network requests.
    pub fn retry_hltb(&self) {
        if let Ok(mut state) = self.hltb.lock()
            && matches!(*state, HltbState::Unavailable(_))
        {
            *state = HltbState::Pending;
        }
    }

    /// Rendering reads only the shared snapshot; cache and network I/O stay on
    /// the worker. Failed lookups can be retried with `,ct` or a library reload.
    pub fn get_hltb(&self) -> HltbState {
        self.hltb
            .lock()
            .map(|state| state.clone())
            .unwrap_or_else(|_| HltbState::Unavailable("HLTB state unavailable".into()))
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
                if let Ok(mut slot) = reference.lock() {
                    *slot = Some(info);
                }
            }
        });
    }

    pub fn get_developer(&self) -> String {
        if let Ok(guard) = self.info.lock() {
            if let Some(info) = &*guard {
                if !info.developers.is_empty() {
                    return info.developers.join(", ");
                }
            }
        }
        self.developer.clone()
    }

    pub fn get_publisher(&self) -> String {
        if let Ok(guard) = self.info.lock() {
            if let Some(info) = &*guard {
                if !info.publishers.is_empty() {
                    return info.publishers.join(", ");
                }
            }
        }
        self.publisher.clone()
    }

    pub fn get_description(&self) -> String {
        if let Ok(guard) = self.info.lock() {
            if let Some(info) = &*guard {
                return info.description.clone();
            }
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
        if let Ok(guard) = self.status.lock() {
            if let Some(live) = &*guard {
                return Some(live.clone());
            }
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
        if let Ok(mut status) = self.status.lock() {
            *status = Some(new_status);
        }
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

#[cfg(test)]
mod hltb_tests {
    use super::*;
    use crate::interface::hltb::HltbData;
    use tui::{Terminal, backend::TestBackend};

    fn portal() -> Game {
        Game::from_library(LibraryGameJson {
            app_id: 400,
            name: "Portal".into(),
            is_installed: true,
            install_path: Some("/games/Portal".into()),
            update_available: false,
            is_owned: true,
            is_family_shared: false,
            platform: None,
            active_branch: None,
            assets: None,
            store_url: Some("https://store.steampowered.com/app/400".into()),
        })
    }

    #[test]
    fn cached_games_restore_pending_state_and_clones_share_live_results() {
        let game = portal();
        let restored: Game = serde_json::from_str(&serde_json::to_string(&game).unwrap()).unwrap();
        assert!(matches!(restored.get_hltb(), HltbState::Pending));
        let clone = game.clone();
        *clone.hltb.lock().unwrap() = HltbState::Unavailable("HTTP 429".into());
        assert!(matches!(game.get_hltb(), HltbState::Unavailable(e) if e == "HTTP 429"));
        game.query_hltb();
        assert!(
            matches!(game.get_hltb(), HltbState::Unavailable(_)),
            "failed lookups do not retry every frame"
        );
    }

    #[test]
    fn browsing_remains_available_during_lookup_and_failures_can_be_retried() {
        let _serial = hltb::WORKER_TEST_LOCK.lock().unwrap();
        let game = portal();
        let mut other = portal();
        other.id = 620;
        other.name = "Portal 2".into();
        let mut browser = crate::browse::Browser::new(vec![game.clone(), other.clone()]);
        let (release, blocked) = std::sync::mpsc::channel();
        game.query_hltb_with(move |_, _, _| {
            // A bounded gate keeps a blocking regression from hanging the suite.
            let _ = blocked.recv_timeout(std::time::Duration::from_secs(2));
            HltbState::Unavailable("connection lost".into())
        });
        assert!(matches!(game.get_hltb(), HltbState::Loading));
        other.query_hltb_with(|_, _, _| panic!("a busy worker must not queue another lookup"));
        assert!(matches!(other.get_hltb(), HltbState::Pending));
        browser.select_index(1);
        assert_eq!(browser.selected().unwrap().id, 620);
        assert!(
            browser
                .action_candidates()
                .iter()
                .any(|row| row.action == crate::browse::Action::Play)
        );
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|f| {
                f.render_widget(
                    crate::ui::detail::detail(Some(&game), false, 60, 22),
                    f.size(),
                )
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let text = (0..24)
            .flat_map(|y| (0..80).map(move |x| buffer.get(x, y).symbol.as_str()))
            .collect::<String>();
        assert!(
            !text.contains("HLTB"),
            "a first lookup should not add a loading row"
        );
        release.send(()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while matches!(game.get_hltb(), HltbState::Loading) {
            assert!(
                std::time::Instant::now() < deadline,
                "worker did not publish its failure"
            );
            thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(matches!(game.get_hltb(), HltbState::Unavailable(_)));
        terminal
            .draw(|f| {
                f.render_widget(
                    crate::ui::detail::detail(Some(&game), false, 60, 22),
                    f.size(),
                )
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert!((0..24).any(|y| {
            (0..80)
                .map(|x| buffer.get(x, y).symbol.as_str())
                .collect::<String>()
                .contains("Fetch Failed")
        }));

        browser.select_index(0);
        browser.open_hltb();
        assert!(
            browser
                .notice
                .as_deref()
                .unwrap()
                .contains("retry requested")
        );
        assert!(matches!(game.get_hltb(), HltbState::Pending));
        // Retry without a library refresh, credentials, or any real network.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while matches!(game.get_hltb(), HltbState::Pending) {
            assert!(
                std::time::Instant::now() < deadline,
                "failed worker retained its permit"
            );
            game.query_hltb_with(|_, _, _| HltbState::NotFound);
            thread::sleep(std::time::Duration::from_millis(5));
        }
        while matches!(game.get_hltb(), HltbState::Loading) {
            assert!(std::time::Instant::now() < deadline);
            thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(matches!(game.get_hltb(), HltbState::NotFound));
        game.retry_hltb();
        assert!(
            matches!(game.get_hltb(), HltbState::NotFound),
            "successful misses retain their cache policy"
        );
        // Publishing the snapshot precedes the worker's final permit drop.
        // Wait for that drop before another test exercises the global permit.
        loop {
            if let Some(permit) = hltb::reserve() {
                drop(permit);
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn expired_estimates_remain_visible_until_the_worker_publishes_fresh_times() {
        let _serial = hltb::WORKER_TEST_LOCK.lock().unwrap();
        let game = portal();
        let previous = HltbData {
            game_id: 7230,
            game_name: "Portal".into(),
            main: Some(11230),
            extra: Some(18943),
            completionist: Some(37927),
        };
        let fresh = HltbData {
            main: Some(14400),
            ..previous.clone()
        };
        let expected = fresh.clone();
        let (published, started) = std::sync::mpsc::channel();
        let (release, blocked) = std::sync::mpsc::channel();
        game.query_hltb_with(move |_, _, publish| {
            publish(previous);
            published.send(()).unwrap();
            blocked
                .recv_timeout(std::time::Duration::from_secs(2))
                .unwrap();
            HltbState::Ready {
                data: fresh,
                stale: false,
            }
        });
        started
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        let clone = game.clone();
        assert!(matches!(
            clone.get_hltb(),
            HltbState::Ready {
                data: HltbData {
                    main: Some(11230),
                    ..
                },
                stale: true
            }
        ));
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|f| {
                f.render_widget(
                    crate::ui::detail::detail(Some(&game), false, 60, 22),
                    f.size(),
                )
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let text = (0..24)
            .flat_map(|y| (0..80).map(move |x| buffer.get(x, y).symbol.as_str()))
            .collect::<String>();
        assert!(text.contains("Cached"));
        assert!(text.contains("3h 7m"));
        assert!(!text.contains("Loading"));
        release.send(()).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            if let HltbState::Ready { data, stale: false } = clone.get_hltb() {
                assert_eq!(data, expected);
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            thread::sleep(std::time::Duration::from_millis(5));
        }
        game.query_hltb_with(|_, _, _| {
            panic!("completed estimates must not be fetched every frame")
        });
        // Do not let the next test race the worker's final permit drop.
        loop {
            if let Some(permit) = hltb::reserve() {
                drop(permit);
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn populated_detail_panes_keep_times_installation_and_description_visible() {
        let game = portal();
        *game.info.lock().unwrap() = Some(InfoJson {
            app_id: 400,
            name: "Portal".into(),
            developers: vec!["Valve".into()],
            publishers: vec!["Valve".into()],
            description: "Aperture Science offers puzzles. ".repeat(40),
            ..Default::default()
        });
        let data = HltbData {
            game_id: 7230,
            game_name: "Portal".into(),
            main: Some(11230),
            extra: Some(18943),
            completionist: Some(37927),
        };
        for stale in [false, true] {
            *game.hltb.lock().unwrap() = HltbState::Ready {
                data: data.clone(),
                stale,
            };
            for (width, height) in [(44, 20), (80, 24), (100, 36)] {
                let value_width = (width - 2) * 82 / 100 - 1;
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                terminal
                    .draw(|f| {
                        f.render_widget(
                            crate::ui::detail::detail(Some(&game), false, value_width, height - 2),
                            f.size(),
                        )
                    })
                    .unwrap();
                let buffer = terminal.backend().buffer();
                let text = (0..height)
                    .map(|y| {
                        (0..width)
                            .map(|x| buffer.get(x, y).symbol.clone())
                            .collect::<String>()
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                for expected in [
                    "HLTB",
                    "3h 7m",
                    "5h 16m",
                    "10h 32m",
                    "Valve",
                    "/games/Portal",
                    "Aperture",
                    "…",
                ] {
                    assert!(
                        text.contains(expected),
                        "{width}x{height} (stale={stale}) missing {expected}:\n{text}"
                    );
                }
                if stale {
                    assert!(text.contains("Cached"));
                }
            }
        }
    }
}
