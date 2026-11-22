//! Install picker options and language change.

use std::collections::HashMap;
use std::thread;

use super::Browser;
use crate::interface::game::Game;
use crate::interface::install_options::{
    self, DryRunJson, InstallOpts, PlatformChoice, language_label,
};
use crate::util::stateful::Named;

/// Platform and language rows of the picker.
#[derive(Debug, Clone, Default)]
pub struct InstallPickerOptions {
    pub platform: PlatformChoice,
    /// Dry runs per platform; `None` failed.
    pub dry_runs: HashMap<PlatformChoice, Option<DryRunJson>>,
    /// The chosen Steam language name.
    pub language: Option<String>,
}

impl InstallPickerOptions {
    /// Dry run for the current platform.
    pub fn current(&self) -> Option<&DryRunJson> {
        self.dry_runs.get(&self.platform).and_then(Option::as_ref)
    }

    /// Languages worth choosing between.
    pub fn languages(&self) -> &[String] {
        match self.current() {
            Some(dry) if dry.offers_languages() => &dry.languages,
            _ => &[],
        }
    }

    /// Explicit platform without any depots.
    pub fn blocked(&self) -> bool {
        self.platform != PlatformChoice::Auto && self.current().is_some_and(|d| !d.has_build())
    }

    /// The install flags to pass.
    pub fn opts(&self) -> InstallOpts {
        let language = if self.languages().is_empty() {
            None
        } else {
            self.language.clone()
        };
        InstallOpts {
            platform: self.platform.arg().map(str::to_string),
            language,
        }
    }

    /// Store a dry run, keep language valid.
    pub fn adopt(&mut self, platform: PlatformChoice, dry: Option<DryRunJson>) {
        self.dry_runs.insert(platform, dry);
        self.sync_language();
    }

    /// Keep choice, else use default.
    fn sync_language(&mut self) {
        let langs = self.languages();
        if langs.is_empty() {
            return;
        }
        let keep = self.language.as_ref().is_some_and(|l| langs.contains(l));
        if !keep {
            let dry = self.current();
            let default = dry
                .and_then(|d| d.default_language.clone())
                .filter(|l| langs.contains(l));
            self.language = default.or_else(|| langs.first().cloned());
        }
    }

    /// Step the language choice.
    pub fn cycle_language(&mut self, forward: bool) {
        let langs = self.languages().to_vec();
        if langs.is_empty() {
            return;
        }
        let pos = self
            .language
            .as_ref()
            .and_then(|l| langs.iter().position(|x| x == l))
            .unwrap_or(0);
        let next = if forward {
            (pos + 1) % langs.len()
        } else {
            (pos + langs.len() - 1) % langs.len()
        };
        self.language = Some(langs[next].clone());
    }
}

/// The change-language overlay state.
#[derive(Debug, Clone, Default)]
pub struct LanguagePicker {
    pub app_id: i32,
    pub game_name: String,
    pub languages: Vec<String>,
    /// The installed language, not offered.
    pub current: Option<String>,
    pub index: usize,
}

impl LanguagePicker {
    /// Build from a dry run.
    pub fn from_dry_run(app_id: i32, game_name: String, dry: &DryRunJson) -> Option<Self> {
        if !dry.offers_languages() {
            return None;
        }
        let current = dry.installed_language.clone();
        let preselect = current.as_ref().or(dry.default_language.as_ref());
        let index = preselect
            .and_then(|c| dry.languages.iter().position(|l| l == c))
            .unwrap_or(0);
        Some(Self {
            app_id,
            game_name,
            languages: dry.languages.clone(),
            current,
            index,
        })
    }

    pub fn selected(&self) -> Option<&String> {
        self.languages.get(self.index)
    }

    pub fn is_current(&self, lang: &str) -> bool {
        self.current.as_deref() == Some(lang)
    }

    pub fn next(&mut self) {
        if self.index + 1 < self.languages.len() {
            self.index += 1;
        }
    }

    pub fn previous(&mut self) {
        self.index = self.index.saturating_sub(1);
    }
}

impl Browser {
    /// Fetch picker options for `app_id`.
    pub(super) fn load_install_options(&mut self, app_id: i32) {
        self.install_options = InstallPickerOptions::default();
        self.install_options_app = app_id;
        self.fetch_install_platform();
    }

    /// Dry-run the current platform if needed.
    fn fetch_install_platform(&mut self) {
        let platform = self.install_options.platform;
        if !self.install_options.dry_runs.contains_key(&platform) {
            let dry = install_options::dry_run(self.install_options_app, platform.arg()).ok();
            self.install_options.adopt(platform, dry);
        }
        self.install_estimate = self
            .install_options
            .current()
            .map(|d| d.disk_size)
            .filter(|&b| b > 0);
    }

    /// Cycle auto / windows / linux.
    pub fn install_picker_cycle_platform(&mut self) {
        self.install_options.platform = self.install_options.platform.next();
        self.fetch_install_platform();
        self.install_options.sync_language();
    }

    /// Step the picker language.
    pub fn install_picker_cycle_language(&mut self, forward: bool) {
        self.install_options.cycle_language(forward);
    }

    /// Remember flags for this install.
    pub fn set_install_opts(&mut self, app_id: i32, opts: InstallOpts) {
        self.install_opts_choice.insert(app_id, opts);
    }

    /// Flags a tracked install uses.
    pub fn install_opts_for(&self, app_id: i32) -> InstallOpts {
        self.install_opts_choice
            .get(&app_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Open the change-language picker.
    pub fn open_language_change(&mut self, game: &Game) {
        let platform = PlatformChoice::from_installed(game.platform.as_deref());
        match install_options::dry_run(game.id, platform.arg()) {
            Ok(dry) => self.adopt_language_dry_run(game, &dry),
            Err(err) => self.notice = Some(format!("Language list failed: {err}")),
        }
    }

    /// Open the picker or explain why not.
    pub fn adopt_language_dry_run(&mut self, game: &Game, dry: &DryRunJson) {
        match LanguagePicker::from_dry_run(game.id, game.get_name(), dry) {
            Some(picker) => self.language_picker = Some(picker),
            None => {
                self.notice = Some(format!(
                    "{} is only available in one language.",
                    game.get_name()
                ))
            }
        }
    }

    pub fn close_language_change(&mut self) {
        self.language_picker = None;
    }

    /// Start the switch; `false` when unchanged.
    pub fn confirm_language_change(&mut self) -> bool {
        let Some(picker) = self.language_picker.as_ref() else {
            return false;
        };
        let Some(lang) = picker.selected().cloned() else {
            return false;
        };
        if picker.is_current(&lang) {
            self.notice = Some(format!("Already installed in {}.", language_label(&lang)));
            return false;
        }
        let app_id = picker.app_id;
        self.language_picker = None;
        if let Some(game) = self.items.iter().find(|g| g.id == app_id) {
            let status = game.status_counter();
            thread::spawn(move || install_options::set_language(app_id, lang, status));
        }
        true
    }
}
