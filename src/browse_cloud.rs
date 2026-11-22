//! Browser hooks for Steam Cloud sync.

use crate::browse::Browser;
use crate::interface::aurelia::CloudDirection;
use crate::interface::cloud_status;
use crate::interface::cloud_sync::{self, ConflictPrompt, GameCloudSync, Resolve};
use crate::util::stateful::Named;

impl Browser {
    /// Manual sync from the overlay.
    pub fn run_cloud_sync(
        &mut self,
        app_id: i32,
        direction: CloudDirection,
        resolve: Option<Resolve>,
    ) {
        cloud_status::mark_syncing(app_id);
        match cloud_sync::sync(app_id, direction, resolve) {
            Ok(result) => {
                cloud_status::record_result(app_id, &result);
                // Only prompt on the detect pass.
                if resolve.is_none() && result.needs_resolution() {
                    cloud_sync::post_prompt(ConflictPrompt {
                        app_id,
                        direction,
                        conflicts: result.conflicts.clone(),
                    });
                }
                self.refresh_cloud(app_id);
                if self.cloud_status.is_empty() {
                    self.cloud_status = result.summary();
                }
            }
            Err(err) => {
                cloud_status::record_error(app_id, &err.to_string());
                self.cloud_status = format!("Failed: {err}");
            }
        }
    }

    /// Answer the open conflict prompt.
    pub fn answer_cloud_conflict(&mut self, choice: Option<Resolve>) {
        let Some(prompt) = cloud_sync::answer(choice) else {
            return;
        };
        match choice {
            Some(side) => self.run_cloud_sync(prompt.app_id, prompt.direction, Some(side)),
            None => self.cloud_status = "Conflict left; both copies kept.".to_string(),
        }
    }

    /// Name of a listed game.
    pub fn cloud_game_name(&self, app_id: i32) -> Option<String> {
        if let Some(game) = self.selected().filter(|g| g.id == app_id) {
            return Some(game.get_name());
        }
        self.visible()
            .into_iter()
            .find(|g| g.id == app_id)
            .map(|g| g.get_name())
    }

    /// Flip Aurelia's global cloud switch.
    pub fn toggle_cloud_sync_global(&mut self) {
        let on = self
            .config_info
            .as_ref()
            .map(|c| c.enable_cloud_sync)
            .unwrap_or(true);
        if let Err(err) = cloud_sync::set_global(!on) {
            self.notice = Some(format!("Cloud sync setting: {err}"));
        }
        let _ = self.open_config();
    }
}

/// Cycle a game's switch; returns status.
pub fn cycle_game_cloud(app_id: i32, current: &mut Option<GameCloudSync>) -> String {
    let next = current
        .as_ref()
        .map(|c| c.mode())
        .unwrap_or(cloud_sync::GameCloudMode::Auto)
        .next();
    match cloud_sync::set_game(app_id, next) {
        Ok(state) => {
            let msg = format!("Cloud sync set to {}.", state.label());
            *current = Some(state);
            msg
        }
        Err(err) => format!("Failed: {err}"),
    }
}
