//! Browser hooks for library info.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::thread;

use super::{Browser, Game};
use crate::interface::aurelia;
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
        match self.selected_version_depot() {
            Some(depot) if depot != 0 => {
                info::open_url(info::steamdb_depot_manifests_url(depot));
                self.versions_status = format!("Opening SteamDB manifests for depot {depot}…");
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

impl Browser {
    /// Detail scroll for the shown game.
    pub fn detail_scroll_for(&self, app_id: Option<i32>) -> u16 {
        match app_id {
            Some(id) if self.expand_description && self.detail_scroll.0 == id => self
                .detail_scroll
                .1
                .min(crate::ui::library_info::max_scroll()),
            _ => 0,
        }
    }

    /// Scroll expanded detail by pages.
    pub fn detail_scroll_by(&mut self, pages: i32) {
        let delta = pages * i32::from(crate::ui::library_info::page());
        let Some(game) = self.selected() else {
            return;
        };
        let current = i32::from(self.detail_scroll_for(Some(game.id)));
        let max = i32::from(crate::ui::library_info::max_scroll());
        let next = (current + delta).clamp(0, max);
        self.detail_scroll = (game.id, next as u16);
    }
}

/// One Versions overlay row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionRow {
    pub depot_id: u32,
    pub depot_name: Option<String>,
    /// `None` for unlisted depots.
    pub branch: Option<String>,
    pub manifest_id: Option<u64>,
    pub size: Option<u64>,
    /// Installed manifest for this depot.
    pub installed: Option<u64>,
    /// Pinned manifest for this depot.
    pub pinned: Option<u64>,
}

impl VersionRow {
    /// Row shows the installed manifest.
    pub fn is_installed(&self) -> bool {
        self.manifest_id.is_some() && self.manifest_id == self.installed
    }
}

/// Action waiting for `y`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionsConfirm {
    Pin,
    Downgrade { depot: u32, manifest: u64 },
}

impl VersionsConfirm {
    /// Prompt shown in the footer.
    pub fn prompt(self) -> String {
        match self {
            VersionsConfirm::Pin => "Pin all depots at installed manifests?".into(),
            VersionsConfirm::Downgrade { depot, manifest } => {
                format!("Downgrade depot {depot} to manifest {manifest}?")
            }
        }
    }
}

impl Browser {
    /// Listed manifests plus unlisted pinned/installed depots.
    pub fn versions_rows(&self) -> Vec<VersionRow> {
        // `list` fills this only with -u.
        let installed = self
            .items
            .iter()
            .find(|g| g.id == self.versions_app_id)
            .map(|g| g.library.local_manifest_ids.clone())
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| self.versions_installed.clone());
        let pinned = self
            .versions_available
            .as_ref()
            .map(|a| a.pinned_manifests.clone())
            .unwrap_or_default();
        let lookup = |map: &BTreeMap<String, u64>, depot: u32| map.get(&depot.to_string()).copied();
        let mut rows: Vec<VersionRow> = self
            .versions_manifests
            .iter()
            .map(|m| VersionRow {
                depot_id: m.depot_id,
                depot_name: m.depot_name.clone().filter(|n| !n.is_empty()),
                branch: Some(m.branch.clone()).filter(|b| !b.is_empty()),
                manifest_id: Some(m.manifest_id),
                size: Some(m.size),
                installed: lookup(&installed, m.depot_id),
                pinned: lookup(&pinned, m.depot_id),
            })
            .collect();
        let listed: BTreeSet<u32> = rows.iter().map(|r| r.depot_id).collect();
        let extra: BTreeSet<u32> = pinned
            .keys()
            .chain(installed.keys())
            .filter_map(|k| k.parse().ok())
            .filter(|d| !listed.contains(d))
            .collect();
        rows.extend(extra.into_iter().map(|depot| VersionRow {
            depot_id: depot,
            depot_name: None,
            branch: None,
            manifest_id: None,
            size: None,
            installed: lookup(&installed, depot),
            pinned: lookup(&pinned, depot),
        }));
        rows
    }

    /// Appmanifest manifests for an installed game.
    pub(super) fn disk_installed_manifests(&self, app_id: i32) -> BTreeMap<String, u64> {
        self.items
            .iter()
            .find(|g| g.id == app_id && !g.install_dir.is_empty())
            .map(|g| info::read_installed_manifests(&g.install_dir, app_id))
            .unwrap_or_default()
    }

    /// Jump to first or last row.
    pub fn versions_jump(&mut self, end: bool) {
        let len = self.versions_rows().len();
        self.versions_index = if end { len.saturating_sub(1) } else { 0 };
    }

    /// Depot of the highlighted row.
    pub fn selected_version_depot(&self) -> Option<u32> {
        self.versions_rows()
            .get(self.versions_index)
            .map(|r| r.depot_id)
    }

    /// Ask before pinning.
    pub fn request_pin(&mut self) {
        self.versions_confirm = Some(VersionsConfirm::Pin);
    }

    /// Ask before downgrading.
    pub(super) fn request_downgrade(&mut self, depot: u32, manifest: u64) {
        self.versions_confirm = Some(VersionsConfirm::Downgrade { depot, manifest });
    }

    /// `y` runs the action; else cancel.
    pub fn versions_confirm_key(&mut self, confirmed: bool) {
        let Some(action) = self.versions_confirm.take() else {
            return;
        };
        if !confirmed {
            self.versions_status = "Cancelled.".into();
            return;
        }
        match action {
            VersionsConfirm::Pin => self.pin_current(),
            VersionsConfirm::Downgrade { depot, manifest } => {
                let app_id = self.versions_app_id;
                let busy = Arc::clone(&self.versions_busy);
                self.versions_status = format!("Downgrading depot {depot} → manifest {manifest}…");
                thread::spawn(move || {
                    aurelia::downgrade(app_id, &[(depot, manifest)], None, false, false, busy);
                });
            }
        }
    }
}
