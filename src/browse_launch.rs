//! Browser hooks for saved launch targets.

use super::Browser;
use crate::config::Config;
use crate::interface::launch;

/// Steam-integration flag for `app_id`.
pub(super) fn steam_flag(app_id: i32) -> bool {
    launch::steam_integration(&Config::cached(), app_id)
}

impl Browser {
    /// Load targets, highlight saved or default.
    pub(super) fn fetch_launch_targets(&mut self, app_id: i32) {
        let config = Config::cached();
        let steam = launch::steam_integration(&config, app_id);
        self.launch.app_id = app_id;
        self.launch.saved = launch::saved_target(&config, app_id).map(str::to_owned);
        self.launch_targets = match launch::launch_targets(app_id, steam) {
            Ok(targets) => targets,
            Err(e) => {
                self.launch_targets_status = format!("Failed: {e}");
                Vec::new()
            }
        };
        let saved = self.launch.saved.as_deref();
        self.launch_targets_index = self
            .launch_targets
            .iter()
            .position(|t| Some(t.id.as_str()) == saved && t.available)
            .or_else(|| self.launch_targets.iter().position(|t| t.default))
            .unwrap_or(0);
        if let Some(saved) = saved
            && !self
                .launch_targets
                .iter()
                .any(|t| t.id == saved && t.available)
        {
            self.launch_targets_status =
                format!("Saved target {saved} is unavailable; Play uses the default.");
        }
    }

    /// Save the highlighted target for Play.
    pub fn launch_target_set_default(&mut self) {
        let Some(target) = self.launch_targets.get(self.launch_targets_index) else {
            return;
        };
        if !target.available {
            self.launch_targets_status = "Can't save an unavailable target.".to_string();
            return;
        }
        let (id, label) = (target.id.clone(), target.label());
        self.launch_targets_status = match launch::store_saved_target(self.launch.app_id, Some(&id))
        {
            Ok(()) => {
                self.launch.saved = Some(id);
                format!("Play now uses {label}.")
            }
            Err(e) => format!("Failed: {e}"),
        };
    }

    /// Clear the saved target (automatic).
    pub fn launch_target_set_automatic(&mut self) {
        self.launch_targets_status = match launch::store_saved_target(self.launch.app_id, None) {
            Ok(()) => {
                self.launch.saved = None;
                "Play picks the target automatically.".to_string()
            }
            Err(e) => format!("Failed: {e}"),
        };
    }

    /// Highlighted target lacks the runtime.
    pub fn launch_target_needs_runtime(&self) -> bool {
        self.launch_targets
            .get(self.launch_targets_index)
            .is_some_and(|t| t.missing.as_deref() == Some("steam-runtime"))
    }

    /// Start `steam-runtime install` when it's missing.
    pub fn launch_target_install_runtime(&mut self) {
        if !self.launch_target_needs_runtime() {
            self.launch_targets_status = "This target doesn't need the Steam runtime.".to_string();
            return;
        }
        if !self.launch.runtime.start() {
            self.launch_targets_status =
                "The Steam runtime install is already running.".to_string();
            return;
        }
        self.launch_targets_status.clear();
    }

    /// Adopt finished install, refresh targets.
    pub fn poll_launch_runtime(&mut self) {
        let Some(result) = self.launch.runtime.take_finished() else {
            return;
        };
        let message = match &result {
            Ok(()) => "Steam runtime installed.".to_string(),
            Err(e) => format!("Steam runtime install failed: {e}"),
        };
        if self.show_launch_targets {
            if result.is_ok() {
                self.fetch_launch_targets(self.launch.app_id);
            }
            self.launch_targets_status = message;
        } else {
            self.notice = Some(message);
        }
    }

    /// Show worker notices in the footer.
    pub fn poll_launch_notices(&mut self) {
        if let Some(notice) = launch::take_notice() {
            self.notice = Some(notice);
        }
    }

    /// Whether the settings game is family-shared.
    pub fn game_config_family_shared(&self) -> bool {
        self.items
            .iter()
            .any(|g| g.id == self.game_config_app_id && g.family_shared)
    }

    /// Game settings row value.
    pub fn steam_integration_label(&self) -> String {
        if self.game_config_family_shared() {
            "on (forced: family-shared)".to_string()
        } else if launch::steam_integration(&Config::cached(), self.game_config_app_id) {
            "on".to_string()
        } else {
            "off".to_string()
        }
    }

    /// Flip the Steam-integration flag.
    pub(super) fn toggle_steam_integration(&mut self) {
        if self.game_config_family_shared() {
            self.game_config_status = "Family-shared games always launch with Steam.".to_string();
            return;
        }
        let id = self.game_config_app_id;
        let on = !launch::steam_integration(&Config::cached(), id);
        self.game_config_status = match launch::store_steam_integration(id, on) {
            Ok(()) if on => "Steam integration on (play --steam).".to_string(),
            Ok(()) => "Steam integration off.".to_string(),
            Err(e) => format!("Failed: {e}"),
        };
    }
}
