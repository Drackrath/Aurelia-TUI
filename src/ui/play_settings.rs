//! Play settings rows for two overlays.

use crate::browse::{Browser, ROW_LAUNCH_OPTIONS, ROW_PREFIX};

/// Value text, or the input with caret.
fn editable(browser: &Browser, row: usize, value: String) -> String {
    match &browser.game_config_input {
        Some(input) if browser.game_config_index == row => format!("{input}_"),
        _ => value,
    }
}

/// Launch options and prefix rows.
pub fn game_config_rows(browser: &Browser) -> Vec<(&'static str, String)> {
    let launch = browser.game_config.as_ref().map(|c| &c.launch);
    let options = match launch.map(|l| l.launch_options.as_str()) {
        Some(line) if !line.is_empty() => line.to_string(),
        _ => "(none)".to_string(),
    };
    let prefix = launch
        .and_then(|l| l.prefix_dir.clone())
        .unwrap_or_else(|| "(Steam compatdata)".to_string());
    vec![
        (
            "Launch options",
            editable(browser, ROW_LAUNCH_OPTIONS, options),
        ),
        ("Wine prefix", editable(browser, ROW_PREFIX, prefix)),
    ]
}

/// The Wine prefix launches use.
pub fn prefix_in_use(browser: &Browser) -> Option<String> {
    let launch = &browser.game_config.as_ref()?.launch;
    let path = launch.wine_prefix.clone()?;
    let custom = if launch.prefix_dir.is_some() {
        " (custom)"
    } else {
        ""
    };
    Some(format!("Prefix in use: {path}{custom}"))
}

/// Hint line while editing text.
pub fn edit_hint(browser: &Browser) -> Option<String> {
    browser.game_config_input.as_ref()?;
    let extra = if browser.game_config_index == ROW_PREFIX {
        " · ~ expands"
    } else {
        " · e.g. VAR=1 %command% -arg"
    };
    Some(format!("[Enter] save · [Esc] cancel · empty clears{extra}"))
}

/// Global Settings rows from `config show`.
pub fn settings_rows(browser: &Browser) -> Vec<(&'static str, String)> {
    let Some(info) = &browser.config_info else {
        return Vec::new();
    };
    let on_off = |on: bool| if on { "on" } else { "off" }.to_string();
    vec![
        (
            "Shader pre-caching",
            if info.play.shaders_auto() {
                "auto"
            } else {
                "off"
            }
            .to_string(),
        ),
        (
            "Anti-cheat auto-install",
            on_off(info.play.anticheat_auto_install),
        ),
    ]
}
