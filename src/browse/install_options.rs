//! Install picker options and language change.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::thread;

use super::Browser;
use crate::interface::aurelia;
use crate::interface::game::Game;
use crate::interface::install_options::{
    self, DryRunJson, InstallOpts, PlatformChoice, language_label,
};
use crate::util::stateful::Named;

/// Background results for the options.
#[derive(Debug, Clone)]
pub enum OptionsMsg {
    /// A picker dry run finished.
    Sized {
        app_id: i32,
        platform: PlatformChoice,
        dry: Option<DryRunJson>,
    },
    /// Store platforms from `info`.
    Platforms { app_id: i32, platforms: Vec<String> },
    /// Change-language dry run finished.
    Languages {
        app_id: i32,
        result: Result<DryRunJson, String>,
    },
}

/// Shared inbox for option workers.
pub type OptionsInbox = Arc<Mutex<Vec<OptionsMsg>>>;

/// Platform and language rows of the picker.
#[derive(Debug, Clone, Default)]
pub struct InstallPickerOptions {
    pub platform: PlatformChoice,
    /// Dry runs per platform; `None` failed.
    pub dry_runs: HashMap<PlatformChoice, Option<DryRunJson>>,
    /// Platforms still being sized.
    pub pending: HashSet<PlatformChoice>,
    /// Store platforms; `None` unknown.
    pub store_platforms: Option<Vec<String>>,
    /// The chosen Steam language name.
    pub language: Option<String>,
}

impl InstallPickerOptions {
    /// Dry run for the current platform.
    pub fn current(&self) -> Option<&DryRunJson> {
        self.dry_runs.get(&self.platform).and_then(Option::as_ref)
    }

    /// Current platform still sizing.
    pub fn sizing(&self) -> bool {
        self.pending.contains(&self.platform)
    }

    /// Platforms the store says exist.
    pub fn choices(&self) -> Vec<PlatformChoice> {
        let known = self.store_platforms.as_ref().filter(|p| !p.is_empty());
        let has = |name: &str| known.is_none_or(|p| p.iter().any(|x| x.eq_ignore_ascii_case(name)));
        let mut out = vec![PlatformChoice::Auto];
        if has("windows") {
            out.push(PlatformChoice::Windows);
        }
        if has("linux") {
            out.push(PlatformChoice::Linux);
        }
        out
    }

    /// The next offered platform.
    pub fn next_platform(&self) -> PlatformChoice {
        let choices = self.choices();
        let pos = choices.iter().position(|c| *c == self.platform);
        match pos {
            Some(i) => choices[(i + 1) % choices.len()],
            None => PlatformChoice::Auto,
        }
    }

    /// Adopt store platforms, drop unavailable choice.
    pub fn set_store_platforms(&mut self, platforms: Vec<String>) {
        self.store_platforms = Some(platforms);
        if !self.choices().contains(&self.platform) {
            self.platform = PlatformChoice::Auto;
            self.sync_language();
        }
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
        self.pending.remove(&platform);
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
    /// Waiting for the dry run.
    pub loading: bool,
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
            loading: false,
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

/// Whether the game can have a prefix.
pub fn has_wine_prefix(game: &Game) -> bool {
    !game
        .platform
        .as_deref()
        .is_some_and(|p| p.eq_ignore_ascii_case("linux"))
}

impl Browser {
    /// Start picker option workers for `app_id`.
    pub(super) fn load_install_options(&mut self, app_id: i32) {
        self.install_options = InstallPickerOptions::default();
        self.install_options_app = app_id;
        self.install_estimate = None;
        let known = self
            .items
            .iter()
            .find(|g| g.id == app_id)
            .and_then(Game::info_snapshot)
            .map(|i| i.platforms)
            .filter(|p| !p.is_empty());
        match known {
            Some(platforms) => self.install_options.store_platforms = Some(platforms),
            None => {
                let inbox = Arc::clone(&self.options_inbox);
                thread::spawn(move || {
                    let platforms = aurelia::fetch_info(app_id)
                        .map(|i| i.platforms)
                        .unwrap_or_default();
                    push(&inbox, OptionsMsg::Platforms { app_id, platforms });
                });
            }
        }
        self.request_install_dry_run();
    }

    /// Size the current platform off-thread.
    fn request_install_dry_run(&mut self) {
        let platform = self.install_options.platform;
        let opts = &mut self.install_options;
        if opts.dry_runs.contains_key(&platform) || opts.pending.contains(&platform) {
            self.sync_install_estimate();
            return;
        }
        opts.pending.insert(platform);
        self.install_estimate = None;
        let app_id = self.install_options_app;
        let inbox = Arc::clone(&self.options_inbox);
        thread::spawn(move || {
            let dry = install_options::dry_run(app_id, platform.arg()).ok();
            push(
                &inbox,
                OptionsMsg::Sized {
                    app_id,
                    platform,
                    dry,
                },
            );
        });
    }

    fn sync_install_estimate(&mut self) {
        self.install_estimate = self
            .install_options
            .current()
            .map(|d| d.disk_size)
            .filter(|&b| b > 0);
    }

    /// Cycle the offered platforms.
    pub fn install_picker_cycle_platform(&mut self) {
        self.install_options.platform = self.install_options.next_platform();
        self.request_install_dry_run();
        self.install_options.sync_language();
    }

    /// Step the picker language.
    pub fn install_picker_cycle_language(&mut self, forward: bool) {
        self.install_options.cycle_language(forward);
    }

    /// Adopt finished option workers.
    pub fn poll_install_options(&mut self) {
        let msgs = match self.options_inbox.lock() {
            Ok(mut guard) => std::mem::take(&mut *guard),
            Err(_) => return,
        };
        for msg in msgs {
            self.apply_options_msg(msg);
        }
    }

    /// Apply one worker result.
    pub fn apply_options_msg(&mut self, msg: OptionsMsg) {
        match msg {
            OptionsMsg::Sized {
                app_id,
                platform,
                dry,
            } if app_id == self.install_options_app => {
                self.install_options.adopt(platform, dry);
                self.sync_install_estimate();
            }
            OptionsMsg::Platforms { app_id, platforms } if app_id == self.install_options_app => {
                self.install_options.set_store_platforms(platforms);
                self.request_install_dry_run();
            }
            OptionsMsg::Languages { app_id, result } => {
                let waiting = self
                    .language_picker
                    .as_ref()
                    .is_some_and(|p| p.loading && p.app_id == app_id);
                if !waiting {
                    return;
                }
                let name = self
                    .language_picker
                    .as_ref()
                    .map(|p| p.game_name.clone())
                    .unwrap_or_default();
                match result {
                    Ok(dry) => self.adopt_language_dry(app_id, name, &dry),
                    Err(err) => {
                        self.language_picker = None;
                        self.notice = Some(format!("Language list failed: {err}"));
                    }
                }
            }
            _ => {}
        }
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

    /// Open the change-language picker, loading.
    pub fn open_language_change(&mut self, game: &Game) {
        self.language_picker = Some(LanguagePicker {
            app_id: game.id,
            game_name: game.get_name(),
            loading: true,
            ..Default::default()
        });
        let platform = PlatformChoice::from_installed(game.platform.as_deref());
        let app_id = game.id;
        let inbox = Arc::clone(&self.options_inbox);
        thread::spawn(move || {
            let result =
                install_options::dry_run(app_id, platform.arg()).map_err(|e| e.to_string());
            push(&inbox, OptionsMsg::Languages { app_id, result });
        });
    }

    /// Open the picker or explain why not.
    pub fn adopt_language_dry_run(&mut self, game: &Game, dry: &DryRunJson) {
        self.adopt_language_dry(game.id, game.get_name(), dry);
    }

    fn adopt_language_dry(&mut self, app_id: i32, name: String, dry: &DryRunJson) {
        match LanguagePicker::from_dry_run(app_id, name.clone(), dry) {
            Some(picker) => self.language_picker = Some(picker),
            None => {
                self.language_picker = None;
                self.notice = Some(format!("{name} is only available in one language."));
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
        if picker.loading {
            return false;
        }
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

    /// Prefix toggle, only when offered.
    pub fn toggle_uninstall_prefix(&mut self) {
        if self.selected().is_some_and(|g| has_wine_prefix(&g)) {
            self.uninstall_delete_prefix = !self.uninstall_delete_prefix;
        }
    }

    /// Whether `y` also deletes the prefix.
    pub fn uninstall_deletes_prefix(&self) -> bool {
        self.uninstall_delete_prefix && self.selected().is_some_and(|g| has_wine_prefix(&g))
    }

    /// Close the uninstall prompt.
    pub fn close_uninstall_prompt(&mut self) {
        self.confirm_uninstall = false;
        self.uninstall_delete_prefix = false;
    }
}

fn push(inbox: &OptionsInbox, msg: OptionsMsg) {
    if let Ok(mut guard) = inbox.lock() {
        guard.push(msg);
    }
}
