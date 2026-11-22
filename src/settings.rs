//! Settings overlay rows and their actions.

use crate::browse::Browser;
use crate::config::Config;
use crossterm::event::KeyCode;

use crate::interface::library_info;
use crate::interface::{aurelia, binary};

/// One changeable Settings row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    Hltb,
    AutoCloudSync,
    StoreLanguage,
    Binary,
    Presence,
    PersonaWait,
    CloudSync,
    DownloadLimit,
    Shaders,
    AntiCheat,
    Proxy,
}

/// How Enter changes a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Toggle,
    Cycle,
    Input,
}

impl Setting {
    /// Row label.
    pub fn label(self) -> &'static str {
        match self {
            Setting::Hltb => "HowLongToBeat",
            Setting::AutoCloudSync => "Sync saves around Play (TUI)",
            Setting::StoreLanguage => "Store language",
            Setting::Binary => "Aurelia binary",
            Setting::Presence => "Presence",
            Setting::PersonaWait => "Persona wait",
            Setting::CloudSync => "Aurelia Steam Cloud sync",
            Setting::DownloadLimit => "Download limit",
            Setting::Shaders => "Shader pre-caching",
            Setting::AntiCheat => "Anti-cheat auto-install",
            Setting::Proxy => "Proxy",
        }
    }

    /// Toggle, cycle or text input.
    pub fn kind(self) -> Kind {
        match self {
            Setting::StoreLanguage => Kind::Cycle,
            Setting::Binary | Setting::PersonaWait | Setting::DownloadLimit | Setting::Proxy => {
                Kind::Input
            }
            _ => Kind::Toggle,
        }
    }

    /// Dim one-line description.
    pub fn help(self) -> &'static str {
        match self {
            Setting::Hltb => "Fetch completion times from HowLongToBeat (TUI only).",
            Setting::AutoCloudSync => "TUI syncs saves before and after Play.",
            Setting::StoreLanguage => "Store text language; Enter opens a list.",
            Setting::Binary => "Path to the aurelia binary; empty = automatic.",
            Setting::Presence => "Friends/chat presence; offline means invisible.",
            Setting::PersonaWait => "Seconds to wait for friends' status, 1-300.",
            Setting::CloudSync => "Aurelia's own Steam Cloud sync, all games.",
            Setting::DownloadLimit => "Download speed cap in kbit/s; off = unlimited.",
            Setting::Shaders => "Replay Vulkan shader caches before launch.",
            Setting::AntiCheat => "Install EAC/BattlEye runtimes when a game needs them.",
            Setting::Proxy => "HTTP(S) proxy URL; empty = direct. [c] clears.",
        }
    }

    /// Old single-letter shortcut.
    pub fn shortcut(self) -> char {
        match self {
            Setting::Hltb => 'h',
            Setting::AutoCloudSync => 't',
            Setting::StoreLanguage => 'l',
            Setting::Binary => 'b',
            Setting::Presence => 'o',
            Setting::PersonaWait => 'w',
            Setting::CloudSync => 'g',
            Setting::DownloadLimit => 'd',
            Setting::Shaders => 's',
            Setting::AntiCheat => 'a',
            Setting::Proxy => 'e',
        }
    }
}

/// TUI-owned rows.
pub const TUI_ROWS: [Setting; 4] = [
    Setting::Hltb,
    Setting::AutoCloudSync,
    Setting::StoreLanguage,
    Setting::Binary,
];

/// Rows needing `config show`.
pub const LAUNCHER_ROWS: [Setting; 6] = [
    Setting::Presence,
    Setting::PersonaWait,
    Setting::CloudSync,
    Setting::Shaders,
    Setting::AntiCheat,
    Setting::Proxy,
];

/// Steam/Aurelia rows shown now.
pub fn steam_rows(browser: &Browser) -> Vec<Setting> {
    if browser.config_info.is_some() {
        let mut rows = LAUNCHER_ROWS.to_vec();
        rows.insert(3, Setting::DownloadLimit);
        return rows;
    }
    // Keep an open proxy edit visible.
    let mut rows = vec![Setting::DownloadLimit];
    if browser.config_proxy_input.is_some() {
        rows.push(Setting::Proxy);
    }
    rows
}

/// Every selectable row, in order.
pub fn selectable(browser: &Browser) -> Vec<Setting> {
    let mut rows = TUI_ROWS.to_vec();
    rows.extend(steam_rows(browser));
    rows
}

/// The highlighted row.
pub fn selected(browser: &Browser) -> Option<Setting> {
    let rows = selectable(browser);
    rows.get(browser.config_index.min(rows.len().saturating_sub(1)))
        .copied()
}

/// Move the highlight down, clamped.
pub fn next(browser: &mut Browser) {
    let n = selectable(browser).len().max(1);
    browser.config_index = (browser.config_index + 1).min(n - 1);
}

/// Move the highlight up, clamped.
pub fn previous(browser: &mut Browser) {
    let n = selectable(browser).len().max(1);
    browser.config_index = browser.config_index.min(n - 1).saturating_sub(1);
}

/// Row behind a shortcut letter.
pub fn from_shortcut(c: char) -> Option<Setting> {
    TUI_ROWS
        .into_iter()
        .chain(LAUNCHER_ROWS)
        .chain([Setting::DownloadLimit])
        .find(|s| s.shortcut() == c)
}

/// Act on the highlighted row.
pub fn activate(browser: &mut Browser, config: &mut Config) {
    if let Some(setting) = selected(browser) {
        apply(setting, browser, config);
    }
}

/// Toggle, cycle, or open the input.
pub fn apply(setting: Setting, browser: &mut Browser, config: &mut Config) {
    // Keep the highlight on it.
    if let Some(i) = selectable(browser).iter().position(|s| *s == setting) {
        browser.config_index = i;
    }
    match setting {
        Setting::Hltb => {
            save(browser, config, "HLTB setting", |c| {
                c.hltb_enabled = !c.hltb_enabled
            });
        }
        Setting::AutoCloudSync => {
            save(browser, config, "Cloud sync setting", |c| {
                c.auto_cloud_sync = !c.auto_cloud_sync
            });
        }
        Setting::StoreLanguage => {
            browser.config_language_pick = Some(language_index(config.info_language.as_deref()))
        }
        Setting::Binary => browser.config_binary_input = Some(binary_prefill(config)),
        Setting::Presence => {
            // Unknown presence counts as offline.
            let online = browser
                .config_info
                .as_ref()
                .map(|c| c.is_online())
                .unwrap_or(false);
            if let Err(e) = aurelia::set_presence(!online) {
                browser.notice = Some(format!("Presence: {e}"));
            }
            let _ = browser.open_config();
        }
        Setting::PersonaWait => browser.begin_persona_wait_edit(),
        Setting::CloudSync => browser.toggle_cloud_sync_global(),
        Setting::DownloadLimit => browser.begin_limit_edit(),
        Setting::Shaders => browser.toggle_shader_precache(),
        Setting::AntiCheat => browser.toggle_anticheat_auto_install(),
        Setting::Proxy => browser.begin_proxy_edit(),
    }
}

/// Save an edited config; true on success.
fn save(
    browser: &mut Browser,
    config: &mut Config,
    what: &str,
    change: impl FnOnce(&mut Config),
) -> bool {
    let mut updated = config.clone();
    change(&mut updated);
    match updated.save() {
        Ok(()) => {
            *config = updated;
            true
        }
        Err(error) => {
            browser.notice = Some(format!("{what}: {error}"));
            false
        }
    }
}

/// Language list; index 0 = default.
pub fn language_choices() -> Vec<Option<&'static str>> {
    std::iter::once(None)
        .chain(library_info::LANGUAGES.iter().copied().map(Some))
        .collect()
}

/// Index of `current` in the choices.
pub fn language_index(current: Option<&str>) -> usize {
    language_choices()
        .iter()
        .position(|l| *l == current)
        .unwrap_or(0)
}

/// Language picker keys; Enter saves changes.
pub fn language_key(browser: &mut Browser, config: &mut Config, key: KeyCode) {
    let Some(i) = browser.config_language_pick else {
        return;
    };
    let last = language_choices().len() - 1;
    match key {
        KeyCode::Esc | KeyCode::Char('q') => browser.config_language_pick = None,
        KeyCode::Down | KeyCode::Char('j') => {
            browser.config_language_pick = Some((i + 1).min(last))
        }
        KeyCode::Up | KeyCode::Char('k') => {
            browser.config_language_pick = Some(i.saturating_sub(1))
        }
        KeyCode::Enter | KeyCode::Char('\n') | KeyCode::Char(' ') => {
            browser.config_language_pick = None;
            let chosen = language_choices()[i].map(str::to_string);
            // Refetch only on real change.
            if chosen != config.info_language
                && save(browser, config, "Language setting", |c| {
                    c.info_language = chosen
                })
            {
                browser.reset_store_info();
            }
        }
        _ => {}
    }
}

/// Binary input starts at the current path.
pub fn binary_prefill(config: &Config) -> String {
    config
        .aurelia_bin
        .clone()
        .filter(|p| !p.trim().is_empty())
        .unwrap_or_else(|| {
            let bin = aurelia::bin();
            binary::resolve(&bin)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or(bin)
        })
}

/// Save the typed binary path.
pub fn commit_binary(browser: &mut Browser, config: &mut Config) {
    let Some(typed) = browser.config_binary_input.take() else {
        return;
    };
    // Untouched prefill keeps automatic mode.
    if typed.trim() == binary_prefill(config) {
        browser.notice = Some("Aurelia binary unchanged.".into());
        return;
    }
    browser.notice = match binary::set_configured(config, &typed) {
        Ok(Some(notice)) => Some(notice),
        Ok(None) if typed.trim().is_empty() => Some("Aurelia binary: automatic.".into()),
        Ok(None) => Some(format!("Aurelia binary: {}", typed.trim())),
        Err(error) => Some(error),
    };
}
