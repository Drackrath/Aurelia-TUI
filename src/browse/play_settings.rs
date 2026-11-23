//! Browser state for play settings.

use super::Browser;
use crate::interface::aurelia;
use crate::interface::play_settings as ps;

/// Game settings rows added here.
pub const PLAY_ROWS: usize = 2;
/// First play row's index.
pub const FIRST_PLAY_ROW: usize = 7;
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

    /// Ask before installing an anti-cheat runtime.
    pub fn ask_anticheat(&mut self, kind: ps::AntiCheat) {
        self.play.confirm_anticheat = Some(kind);
    }

    /// Answer the anti-cheat prompt.
    pub fn answer_anticheat(&mut self, yes: bool) {
        if let Some(kind) = self.play.confirm_anticheat.take()
            && yes
        {
            self.clear_proton_status();
            ps::spawn_anticheat_install(kind, self.proton_status.clone());
        }
    }

    /// Ask before installing the highlighted runtime.
    pub fn ask_proton_install(&mut self) {
        match self.selected_proton() {
            Some(p) if !p.installed => {
                self.play.confirm_proton = Some(ProtonConfirm::Install(p.name.clone()));
            }
            Some(p) => self.notice = Some(format!("{} is already installed.", p.name)),
            None => {}
        }
    }

    /// Ask before changing the global default.
    pub fn ask_proton_default(&mut self) {
        if let Some(p) = self.selected_proton() {
            self.play.confirm_proton = Some(ProtonConfirm::Default(p.name.clone()));
        }
    }

    /// Answer the Proton install/default prompt.
    pub fn answer_proton(&mut self, yes: bool) {
        let Some(ask) = self.play.confirm_proton.take() else {
            return;
        };
        if !yes {
            return;
        }
        match ask {
            ProtonConfirm::Install(name) => {
                self.clear_proton_status();
                let status = self.proton_status.clone();
                std::thread::spawn(move || aurelia::proton_install(name, status));
            }
            ProtonConfirm::Default(name) => {
                if let Err(e) = aurelia::proton_default(&name) {
                    self.notice = Some(format!("Proton default: {e}"));
                }
                // Keep highlight across re-fetch.
                let keep = self.proton_index;
                if self.load_protons().is_ok() {
                    self.proton_index = keep.min(self.protons.len().saturating_sub(1));
                }
            }
        }
    }

    /// Ask before processing a game's shaders.
    pub fn ask_shaders(&mut self, app_id: i32, force: bool) {
        self.play.confirm_shaders = Some((app_id, force));
    }

    /// Answer the shaders prompt.
    pub fn answer_shaders(&mut self, yes: bool) {
        let Some((id, force)) = self.play.confirm_shaders.take() else {
            return;
        };
        if !yes {
            return;
        }
        if let Some(game) = self.items.iter().find(|g| g.id == id) {
            ps::spawn_shaders(id, force, game.status_counter());
        }
    }

    /// Answer whichever play prompt is open.
    pub fn answer_play_confirm(&mut self, yes: bool) {
        if self.play.confirm_proton.is_some() {
            self.answer_proton(yes);
        } else if self.play.confirm_anticheat.is_some() {
            self.answer_anticheat(yes);
        } else {
            self.answer_shaders(yes);
        }
    }

    /// The pending confirm question, if any.
    pub fn play_confirm_prompt(&self) -> Option<String> {
        match &self.play.confirm_proton {
            Some(ProtonConfirm::Install(name)) => {
                return Some(format!("Install {name} (download)?"));
            }
            Some(ProtonConfirm::Default(name)) => {
                return Some(format!("Make {name} the default for all games?"));
            }
            None => {}
        }
        if let Some(kind) = self.play.confirm_anticheat {
            return Some(format!("Install the Proton {}?", kind.label()));
        }
        let (id, force) = self.play.confirm_shaders?;
        let name = self
            .items
            .iter()
            .find(|g| g.id == id)
            .map(|g| g.raw_name().to_string())
            .unwrap_or_else(|| id.to_string());
        let again = if force { " again" } else { "" };
        Some(format!(
            "Process Vulkan shaders for {name}{again}? Can take minutes."
        ))
    }

    /// Fetch, sort and tag Proton runtimes.
    pub(super) fn load_protons(&mut self) -> Result<(), crate::util::error::STError> {
        let (mut protons, default) = aurelia::proton_list_with_default()?;
        ps::natural_sort_protons(&mut protons);
        let names: Vec<String> = protons.iter().map(|p| p.name.clone()).collect();
        let resolved = ps::resolve_default(&default, &names, &ps::compat_tool_dirs());
        for p in protons.iter_mut() {
            p.is_default = resolved.as_deref() == Some(p.name.as_str());
        }
        self.protons = protons;
        self.play.proton_default = default;
        self.play.proton_default_row = resolved;
        Ok(())
    }

    /// The highlighted choice row's options.
    fn choice_options(&self) -> Option<ChoicePicker> {
        let cfg = self.game_config.as_ref();
        let pair = |v: &str| (v.to_string(), v.to_string());
        let (title, options, current): (&str, Vec<(String, String)>, String) =
            match self.game_config_index {
                0 => (
                    "Runner",
                    vec![pair("auto"), pair("umu"), pair("luxtorpeda")],
                    cfg.map(|c| c.runner.clone())
                        .unwrap_or_else(|| "auto".into()),
                ),
                1 => {
                    let mut opts = vec![("(global default)".to_string(), String::new())];
                    opts.extend(self.game_config_protons.iter().map(|p| pair(p)));
                    let cur = cfg.and_then(|c| c.forced_proton_version.clone());
                    // Keep a forced but missing runtime.
                    if let Some(v) = &cur
                        && !opts.iter().any(|(_, o)| o == v)
                    {
                        opts.push((format!("{v} (not installed)"), v.clone()));
                    }
                    ("Forced Proton", opts, cur.unwrap_or_default())
                }
                2 => (
                    "Platform",
                    vec![
                        ("auto".to_string(), String::new()),
                        pair("windows"),
                        pair("linux"),
                    ],
                    cfg.and_then(|c| c.platform_preference.clone())
                        .unwrap_or_default(),
                ),
                3 => (
                    "Steam runtime",
                    vec![pair("auto"), pair("on"), pair("off")],
                    match cfg.and_then(|c| c.steam_runtime_policy.as_deref()) {
                        Some("Enabled") => "on",
                        Some("Disabled") => "off",
                        _ => "auto",
                    }
                    .to_string(),
                ),
                4 => (
                    "Steam prefix",
                    vec![pair("shared"), pair("per-game")],
                    match cfg.and_then(|c| c.steam_prefix_mode.as_deref()) {
                        Some("PerGame") => "per-game",
                        _ => "shared",
                    }
                    .to_string(),
                ),
                _ => return None,
            };
        let current = options
            .iter()
            .position(|(_, v)| v.eq_ignore_ascii_case(&current))
            .unwrap_or(0);
        Some(ChoicePicker {
            row: self.game_config_index,
            title: title.to_string(),
            options,
            current,
            index: current,
        })
    }

    /// Open a choice row picker.
    pub(super) fn open_game_config_picker(&mut self) {
        self.play.picker = self.choice_options();
        self.game_config_status.clear();
    }

    /// Whether the choice picker is open.
    pub fn game_config_picking(&self) -> bool {
        self.play.picker.is_some()
    }

    pub fn picker_next(&mut self) {
        if let Some(p) = self.play.picker.as_mut() {
            p.index = (p.index + 1) % p.options.len().max(1);
        }
    }

    pub fn picker_previous(&mut self) {
        if let Some(p) = self.play.picker.as_mut() {
            let n = p.options.len().max(1);
            p.index = (p.index + n - 1) % n;
        }
    }

    pub fn picker_cancel(&mut self) {
        self.play.picker = None;
    }

    /// Write the picked value to Aurelia.
    pub fn picker_apply(&mut self) {
        let Some(p) = self.play.picker.take() else {
            return;
        };
        let Some((label, value)) = p.options.get(p.index).cloned() else {
            return;
        };
        if p.index == p.current {
            self.game_config_status = format!("{} unchanged.", p.title);
            return;
        }
        let id = self.game_config_app_id;
        let res = match p.row {
            0 => match value.as_str() {
                "umu" => aurelia::config_game_set_umu(id, true),
                "luxtorpeda" => aurelia::config_game_set_native_engine(id, true),
                // --no-umu resets any runner.
                _ => aurelia::config_game_set_umu(id, false),
            },
            1 if value.is_empty() => aurelia::config_game_clear_proton(id),
            1 => aurelia::config_game_set_proton(id, &value),
            2 if value.is_empty() => aurelia::config_game_clear_platform(id),
            2 => aurelia::config_game_set_platform(id, &value),
            3 => aurelia::config_game_set_steam_runtime(id, &value),
            4 => aurelia::config_game_set_steam_prefix_mode(id, &value),
            _ => return,
        };
        self.game_config_status = match res {
            Ok(()) => format!("{} set to {label}.", p.title),
            Err(e) => format!("Failed: {e}"),
        };
        self.refresh_game_config();
    }
}

/// A small list of row choices.
#[derive(Debug, Clone, Default)]
pub struct ChoicePicker {
    pub row: usize,
    pub title: String,
    /// Label and CLI value pairs.
    pub options: Vec<(String, String)>,
    /// Index of the saved value.
    pub current: usize,
    pub index: usize,
}

/// Play settings overlay state.
#[derive(Debug, Clone, Default)]
pub struct PlayUi {
    pub picker: Option<ChoicePicker>,
    pub confirm_anticheat: Option<ps::AntiCheat>,
    pub confirm_proton: Option<ProtonConfirm>,
    /// App id and force flag.
    pub confirm_shaders: Option<(i32, bool)>,
    /// Configured default Proton name.
    pub proton_default: String,
    /// Row the default resolves to.
    pub proton_default_row: Option<String>,
}

/// A pending Proton overlay action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtonConfirm {
    Install(String),
    Default(String),
}
