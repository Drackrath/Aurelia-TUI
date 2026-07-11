//! The settings overlay (modal popup) showing the launcher configuration, with
//! a key to toggle the friends/chat presence online/offline.

use tui::layout::Constraint;
use tui::text::Span;
use tui::widgets::{Cell, Row, Table};

use crate::browse::Browser;
use crate::theme;

/// One key-value row in the settings table.
fn row(label: &str, value: String) -> Row<'static> {
    Row::new(vec![
        Cell::from(Span::styled(label.to_string(), theme::label())),
        Cell::from(Span::styled(value, theme::value())),
    ])
}

/// Render an optional string, falling back to a muted placeholder.
fn opt(value: &Option<String>) -> String {
    match value {
        Some(v) if !v.is_empty() => v.clone(),
        _ => "—".to_string(),
    }
}

/// Render a boolean as a friendly on/off label.
fn yes_no(value: bool) -> String {
    if value { "on" } else { "off" }.to_string()
}

/// The Proxy row value: while editing, the typed URL with a caret; otherwise the
/// configured URL (or a direct-connection placeholder).
fn proxy_value(browser: &Browser) -> String {
    if let Some(input) = &browser.config_proxy_input {
        return format!("{input}_");
    }
    match browser.config_proxy.as_ref().and_then(|p| p.url.as_ref()) {
        Some(url) if !url.is_empty() => url.clone(),
        _ => "— (direct)".to_string(),
    }
}

/// Build the settings overlay content from the fetched launcher configuration.
pub fn config(browser: &Browser) -> Table<'static> {
    let rows = match &browser.config_info {
        Some(info) => vec![
            row("Presence", opt(&info.chat_presence)),
            row("Proton", opt(&info.proton_version)),
            row("Library", opt(&info.steam_library_path)),
            row("Prefix mode", opt(&info.steam_prefix_mode)),
            row("Language", opt(&info.language)),
            row("Cloud sync", yes_no(info.enable_cloud_sync)),
            row("Shared compat data", yes_no(info.use_shared_compat_data)),
            row("Windows discovery", yes_no(info.windows_steam_discovery_enabled)),
            row("Luxtorpeda", yes_no(info.luxtorpeda_enabled)),
            row("Proxy", proxy_value(browser)),
            row(
                "Proxy bypass",
                opt(&browser.config_proxy.as_ref().and_then(|p| p.no_proxy.clone())),
            ),
        ],
        None => vec![Row::new(vec![Cell::from(Span::styled(
            "No configuration",
            theme::value(),
        ))])],
    };

    let title = if browser.config_proxy_input.is_some() {
        "Settings — proxy URL ([Enter] save · [Esc] cancel · empty clears)".to_string()
    } else {
        "Settings ([o] presence · [e] edit proxy · [c] clear proxy)".to_string()
    };

    Table::new(rows)
        .block(theme::panel(title))
        .style(theme::base())
        .widths(&[Constraint::Percentage(32), Constraint::Percentage(68)])
        .column_spacing(1)
}
