//! The per-game settings overlay (modal popup): a game's config overrides —
//! runner (auto/umu/luxtorpeda), forced Proton, platform, Steam runtime policy,
//! Steam prefix mode, and launch script — each a selectable row that Enter cycles.

use tui::layout::Rect;
use tui::text::{Span, Spans};
use tui::widgets::Paragraph;

use crate::browse::Browser;
use crate::theme;
use crate::ui::{paneled_paragraph, selection_marker};

/// Build the game-settings overlay from the browser's open game-config state.
/// Six rows (Runner, Forced Proton, Platform, Steam runtime, Steam prefix,
/// Launch script) show the current value; the highlighted row gets a selection bar. A status/hint line closes it.
pub fn game_config(browser: &Browser, area: Rect) -> Paragraph<'static> {
    let cfg = browser.game_config.as_ref();
    let runner = cfg
        .map(|c| c.runner.clone())
        .unwrap_or_else(|| "auto".to_string());
    let proton = cfg
        .and_then(|c| c.forced_proton_version.clone())
        .unwrap_or_else(|| "(default)".to_string());
    let platform = cfg
        .and_then(|c| c.platform_preference.clone())
        .unwrap_or_else(|| "(auto)".to_string());
    let steam_runtime = match cfg.and_then(|c| c.steam_runtime_policy.as_deref()) {
        Some("Enabled") => "on",
        Some("Disabled") => "off",
        _ => "auto",
    }
    .to_string();
    let steam_prefix = match cfg.and_then(|c| c.steam_prefix_mode.as_deref()) {
        Some("PerGame") => "per-game",
        _ => "shared",
    }
    .to_string();
    let script = match browser.game_script.as_ref() {
        Some(s) if s.exists => s.path.clone().unwrap_or_else(|| "(set)".to_string()),
        _ => "(none)".to_string(),
    };

    let cloud = browser
        .game_cloud_sync
        .as_ref()
        .map(|c| c.label())
        .unwrap_or_else(|| "(unknown)".to_string());

    let mut rows: Vec<(&str, String)> = vec![
        ("Runner", runner),
        ("Forced Proton", proton),
        ("Platform", platform),
        ("Steam runtime", steam_runtime),
        ("Steam prefix", steam_prefix),
        ("Launch script", script),
        ("Steam integration", browser.steam_integration_label()),
    ];
    let row_w = area.width.saturating_sub(2) as usize;
    rows.extend(crate::ui::play_settings::game_config_rows(browser, row_w));
    rows.push(("Aurelia Steam Cloud sync", cloud));

    // Footer first, rows get the rest.
    let inner_w = area.width.saturating_sub(2) as usize;
    let mut footer: Vec<Spans<'static>> = Vec::new();
    if let Some(prefix) = crate::ui::play_settings::prefix_line(browser, inner_w) {
        footer.push(Spans::from(Span::styled(prefix, theme::dim())));
    }
    if !browser.game_config_status.is_empty() {
        footer.push(Spans::from(Span::styled(
            browser.game_config_status.clone(),
            theme::dim(),
        )));
    }
    let parts = crate::ui::play_settings::edit_hint(browser)
        .unwrap_or_else(|| vec!["[Enter] change", "[x] del script", "[Esc] close"]);
    if browser.game_config_status.is_empty() || browser.game_config_editing() {
        for h in crate::ui::play_settings::wrap_hints(&parts, inner_w) {
            footer.push(Spans::from(Span::styled(h, theme::dim())));
        }
    }
    let rows_h = (area.height.saturating_sub(2) as usize).saturating_sub(footer.len());
    let (start, end) = crate::ui::scroll::window(browser.game_config_index, rows.len(), rows_h);

    let mut lines: Vec<Spans<'static>> = Vec::new();
    for (i, (label, value)) in rows.iter().enumerate().take(end).skip(start) {
        let selected = i == browser.game_config_index;
        let style = if selected {
            theme::selection(theme::ACCENT)
        } else {
            theme::value()
        };
        let label_style = if selected { style } else { theme::label() };
        lines.push(Spans::from(vec![
            Span::styled(selection_marker(selected).to_string(), style),
            Span::styled(format!("{label}: "), label_style),
            Span::styled(value.clone(), style),
        ]));
    }
    // Blank spacer only when room.
    if lines.len() + footer.len() < area.height.saturating_sub(2) as usize {
        lines.push(Spans::from(""));
    }
    lines.extend(footer);

    paneled_paragraph(lines, "Game settings".to_string())
}
