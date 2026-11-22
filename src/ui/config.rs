//! Settings overlay: grouped, selectable rows.

use tui::style::Modifier;
use tui::text::{Span, Spans};
use tui::widgets::Paragraph;

use crate::browse::Browser;
use crate::config::Config;
use crate::interface::binary;
use crate::settings::{self, Setting};
use crate::theme;
use crate::ui::{paneled_paragraph, selection_marker};

/// Label column width.
const LABEL_WIDTH: usize = 26;

/// The short default title.
pub const TITLE: &str = "Settings — [j/k] select · [Enter] change · [Esc] close";

/// Optional string, or a dash.
fn opt(value: &Option<String>) -> String {
    match value {
        Some(v) if !v.is_empty() => v.clone(),
        _ => "—".to_string(),
    }
}

/// Boolean as on/off.
fn yes_no(value: bool) -> String {
    if value { "on" } else { "off" }.to_string()
}

/// Proxy URL, or the live edit.
fn proxy_value(browser: &Browser) -> String {
    if let Some(input) = &browser.config_proxy_input {
        return format!("{input}_");
    }
    match browser.config_proxy.as_ref().and_then(|p| p.url.as_ref()) {
        Some(url) if !url.is_empty() => url.clone(),
        _ => "— (direct)".to_string(),
    }
}

/// Presence, noting offline means invisible.
fn presence_value(value: &Option<String>) -> String {
    match value.as_deref() {
        Some("offline") => "offline (invisible)".to_string(),
        _ => opt(value),
    }
}

/// Persona wait, or live edit.
fn persona_wait_value(browser: &Browser, secs: Option<u64>) -> String {
    if let Some(input) = &browser.social.wait_input {
        // Flag out-of-range input early.
        let range = crate::interface::social::parse_persona_wait(input).is_err();
        return format!("{input}_ s{}", if range { "  (1-300)" } else { "" });
    }
    let secs = secs.unwrap_or(crate::interface::social::PERSONA_WAIT_DEFAULT);
    format!("{secs}s (profile status wait)")
}

/// A play-settings value by label.
fn play_value(browser: &Browser, label: &str) -> String {
    crate::ui::play_settings::settings_rows(browser)
        .into_iter()
        .find(|(l, _)| *l == label)
        .map(|(_, v)| v)
        .unwrap_or_else(|| "—".to_string())
}

/// Current value of a row.
pub fn value(browser: &Browser, setting: Setting) -> String {
    let cached = Config::cached();
    let info = browser.config_info.as_ref();
    match setting {
        Setting::Hltb => yes_no(cached.hltb_enabled),
        Setting::AutoCloudSync => yes_no(cached.auto_cloud_sync),
        Setting::StoreLanguage => {
            crate::interface::library_info::language_label(cached.info_language.as_deref())
        }
        Setting::Binary => binary::describe(browser.config_binary_input.as_deref()),
        Setting::Presence => presence_value(&info.and_then(|i| i.chat_presence.clone())),
        Setting::PersonaWait => persona_wait_value(browser, info.and_then(|i| i.persona_wait_secs)),
        Setting::CloudSync => info.map_or_else(|| "—".into(), |i| yes_no(i.enable_cloud_sync)),
        Setting::DownloadLimit => crate::ui::install_queue::limit_value(browser),
        Setting::Shaders => play_value(browser, "Shader pre-caching"),
        Setting::AntiCheat => play_value(browser, "Anti-cheat auto-install"),
        Setting::Proxy => proxy_value(browser),
    }
}

/// Read-only rows from `config show`.
fn info_rows(browser: &Browser) -> Vec<(&'static str, String)> {
    let Some(info) = &browser.config_info else {
        return vec![("Launcher", "Unavailable".into())];
    };
    vec![
        ("Library", opt(&info.steam_library_path)),
        ("Proton", opt(&info.proton_version)),
        ("Prefix mode", opt(&info.steam_prefix_mode)),
        ("Language", opt(&info.language)),
        ("Shared compat data", yes_no(info.use_shared_compat_data)),
        (
            "Windows discovery",
            yes_no(info.windows_steam_discovery_enabled),
        ),
        ("Luxtorpeda", yes_no(info.luxtorpeda_enabled)),
        (
            "Proxy bypass",
            opt(&browser
                .config_proxy
                .as_ref()
                .and_then(|p| p.no_proxy.clone())),
        ),
    ]
}

/// A group header line.
fn header(title: &str) -> Spans<'static> {
    Spans::from(Span::styled(
        title.to_string(),
        theme::accent().add_modifier(Modifier::BOLD),
    ))
}

/// A label/value line.
fn line(
    label: &str,
    key: Option<char>,
    value: String,
    selected: bool,
    readonly: bool,
) -> Spans<'static> {
    let style = if selected {
        theme::selection(theme::ACCENT)
    } else if readonly {
        theme::dim()
    } else {
        theme::value()
    };
    let label_style = if selected || readonly {
        style
    } else {
        theme::label()
    };
    let key = key.map_or_else(|| "    ".to_string(), |c| format!("[{c}] "));
    let key_style = if selected { style } else { theme::dim() };
    Spans::from(vec![
        Span::styled(selection_marker(selected).to_string(), style),
        Span::styled(format!("{label:<LABEL_WIDTH$}"), label_style),
        Span::styled(key, key_style),
        Span::styled(value, style),
    ])
}

/// Append a titled group of rows.
fn group(
    lines: &mut Vec<Spans<'static>>,
    at: &mut usize,
    browser: &Browser,
    title: &str,
    rows: Vec<Setting>,
) {
    if !lines.is_empty() {
        lines.push(Spans::from(""));
    }
    lines.push(header(title));
    let current = settings::selected(browser);
    for setting in rows {
        let selected = current == Some(setting);
        if selected {
            *at = lines.len();
        }
        let value = value(browser, setting);
        let key = Some(setting.shortcut());
        lines.push(line(setting.label(), key, value, selected, false));
    }
}

/// Body lines and the selected line index.
fn body(browser: &Browser) -> (Vec<Spans<'static>>, usize) {
    let mut lines = Vec::new();
    let mut at = 0;
    let tui = settings::TUI_ROWS.to_vec();
    group(&mut lines, &mut at, browser, "TUI", tui);
    let steam = settings::steam_rows(browser);
    group(&mut lines, &mut at, browser, "Steam / Aurelia", steam);
    lines.push(Spans::from(""));
    lines.push(header("Info (read-only)"));
    for (label, value) in info_rows(browser) {
        lines.push(line(label, None, value, false, true));
    }
    (lines, at)
}

/// Title for the current mode.
fn title(browser: &Browser) -> String {
    if browser.config_language_pick.is_some() {
        "Store language — [j/k] select · [Enter] save · [Esc] cancel".to_string()
    } else if browser.config_binary_input.is_some() {
        "Aurelia path — [Enter] save · [Esc] cancel · empty=auto".to_string()
    } else if browser.social.wait_input.is_some() {
        "Persona wait 1-300 s — [Enter] save · [Esc] cancel".to_string()
    } else if browser.config_proxy_input.is_some() {
        "Proxy URL — [Enter] save · [Esc] cancel · empty clears".to_string()
    } else if browser.download_limit.input.is_some() {
        "Download limit — [Enter] save · [Esc] cancel".to_string()
    } else {
        TITLE.to_string()
    }
}

/// Settings overlay, unclipped.
pub fn config(browser: &Browser) -> Paragraph<'static> {
    config_in(browser, u16::MAX)
}

/// Settings overlay sized to `height` rows.
pub fn config_in(browser: &Browser, height: u16) -> Paragraph<'static> {
    if let Some(pick) = browser.config_language_pick {
        return language_picker(pick, height, title(browser));
    }
    let (lines, at) = body(browser);
    // Border plus blank and help.
    let room = usize::from(height).saturating_sub(4).max(1);
    let start = if at < room {
        0
    } else {
        (at + 1 - room).min(lines.len().saturating_sub(room))
    };
    let mut shown: Vec<Spans<'static>> = lines.into_iter().skip(start).take(room).collect();
    shown.push(Spans::from(""));
    if let Some(setting) = settings::selected(browser) {
        shown.push(Spans::from(Span::styled(
            setting.help().to_string(),
            theme::dim(),
        )));
    }
    paneled_paragraph(shown, title(browser))
}

/// Store-language list with a highlight.
fn language_picker(pick: usize, height: u16, title: String) -> Paragraph<'static> {
    let choices = settings::language_choices();
    let room = usize::from(height).saturating_sub(2);
    let (start, end) = crate::ui::scroll::window(pick, choices.len(), room);
    let lines: Vec<Spans<'static>> = (start..end)
        .map(|i| {
            let label = crate::interface::library_info::language_label(choices[i]);
            let selected = i == pick;
            let style = if selected {
                theme::selection(theme::ACCENT)
            } else {
                theme::value()
            };
            Spans::from(vec![
                Span::styled(selection_marker(selected).to_string(), style),
                Span::styled(label, style),
            ])
        })
        .collect();
    paneled_paragraph(lines, title)
}
