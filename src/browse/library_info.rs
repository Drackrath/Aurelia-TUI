//! Browser hooks for library info.

use super::{Browser, Game};
use crate::interface::library_info as info;

/// Most played first, then by name.
pub(super) fn sort_by_playtime(games: &mut [&Game]) {
    games.sort_by(|a, b| {
        b.library
            .playtime_key()
            .cmp(&a.library.playtime_key())
            .then_with(|| {
                a.raw_name()
                    .to_lowercase()
                    .cmp(&b.raw_name().to_lowercase())
            })
    });
}

impl Browser {
    /// Open the selected game on SteamDB.
    pub fn open_steamdb(&mut self) {
        if let Some(game) = self.selected() {
            info::open_url(info::steamdb_app_url(game.id));
            self.notice = Some("Opening SteamDB…".into());
        }
    }

    /// Open highlighted depot's SteamDB manifests.
    pub fn open_steamdb_depot(&mut self) {
        match self.versions_manifests.get(self.versions_index) {
            Some(m) if m.depot_id != 0 => {
                info::open_url(info::steamdb_depot_manifests_url(m.depot_id));
                self.versions_status =
                    format!("Opening SteamDB manifests for depot {}…", m.depot_id);
            }
            _ => self.versions_status = "No depot selected".into(),
        }
    }

    /// Surface browser-open failures.
    pub fn poll_library_info(&mut self) {
        if let Some(error) = info::take_open_error() {
            self.notice = Some(error);
        }
    }

    /// Refetch store info after language change.
    pub fn reset_store_info(&mut self) {
        for game in &self.items {
            game.reset_info();
        }
    }
}
