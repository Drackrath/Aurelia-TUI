//! Browser state for play settings.

use super::Browser;
use crate::interface::aurelia;
use crate::interface::play_settings as ps;

/// Game settings rows added here.
pub const PLAY_ROWS: usize = 2;
/// First play row's index.
pub const FIRST_PLAY_ROW: usize = 6;
/// Row: launch options.
pub const ROW_LAUNCH_OPTIONS: usize = FIRST_PLAY_ROW;
/// Row: custom Wine prefix.
pub const ROW_PREFIX: usize = FIRST_PLAY_ROW + 1;

impl Browser {
    /// Start editing the highlighted play row.
    pub(super) fn play_settings_activate(&mut self) {
        let launch = self.game_config.as_ref().map(|c| c.launch.clone());
        let current = match self.game_config_index {
            ROW_LAUNCH_OPTIONS => launch.map(|l| l.launch_options),
            ROW_PREFIX => launch.and_then(|l| l.prefix_dir),
            _ => return,
        };
        self.game_config_input = Some(current.unwrap_or_default());
        self.game_config_status.clear();
    }

    /// Whether a text row is being edited.
    pub fn game_config_editing(&self) -> bool {
        self.game_config_input.is_some()
    }

    pub fn game_config_input_push(&mut self, c: char) {
        if let Some(s) = self.game_config_input.as_mut() {
            s.push(c);
        }
    }

    pub fn game_config_input_pop(&mut self) {
        if let Some(s) = self.game_config_input.as_mut() {
            s.pop();
        }
    }

    pub fn game_config_input_cancel(&mut self) {
        self.game_config_input = None;
        self.game_config_status.clear();
    }

    /// Save the typed value; keep input on rejection.
    pub fn game_config_input_commit(&mut self) {
        let Some(value) = self.game_config_input.clone() else {
            return;
        };
        let id = self.game_config_app_id;
        let empty = value.trim().is_empty();
        let (res, what) = match self.game_config_index {
            ROW_LAUNCH_OPTIONS => (ps::set_launch_options(id, &value), "Launch options"),
            ROW_PREFIX => (ps::set_prefix(id, &value), "Wine prefix"),
            _ => return,
        };
        match res {
            Ok(()) => {
                self.game_config_input = None;
                self.game_config_status = if empty {
                    format!("{what} cleared.")
                } else {
                    format!("{what} saved.")
                };
                self.refresh_game_config();
            }
            // Show Aurelia's rejection message.
            Err(e) => self.game_config_status = format!("Rejected: {e}"),
        }
    }

    /// Toggle `config shader-precache auto|off`.
    pub fn toggle_shader_precache(&mut self) {
        let auto = self
            .config_info
            .as_ref()
            .map(|c| c.play.shaders_auto())
            .unwrap_or(true);
        if let Err(e) = ps::set_shader_precache(!auto) {
            self.notice = Some(format!("Shader pre-caching: {e}"));
        }
        self.refresh_config_info();
    }

    /// Toggle `config anticheat-auto-install on|off`.
    pub fn toggle_anticheat_auto_install(&mut self) {
        let on = self
            .config_info
            .as_ref()
            .map(|c| c.play.anticheat_auto_install)
            .unwrap_or(true);
        if let Err(e) = ps::set_anticheat_auto_install(!on) {
            self.notice = Some(format!("Anti-cheat auto-install: {e}"));
        }
        self.refresh_config_info();
    }

    /// Re-read `config show`, keeping the proxy.
    fn refresh_config_info(&mut self) {
        if let Ok(info) = aurelia::config_show() {
            self.config_info = Some(info);
        }
    }

    /// Install an anti-cheat runtime from Proton overlay.
    pub fn install_anticheat(&mut self, kind: ps::AntiCheat) {
        self.clear_proton_status();
        ps::spawn_anticheat_install(kind, self.proton_status.clone());
    }
}
