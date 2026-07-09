//! The per-game settings overlay (modal popup): a game's config overrides —
//! runner (auto/umu/luxtorpeda), forced Proton, platform, and launch script —
//! each a selectable row that Enter cycles.

use tui::text::{Span, Spans};
use tui::widgets::Paragraph;

use crate::browse::Browser;
use crate::theme;
use crate::ui::{paneled_paragraph, selection_marker};

/// Build the game-settings overlay from the browser's open game-config state.
/// Four rows (Runner, Forced Proton, Platform, Launch script) show the current
/// value; the highlighted row gets a selection bar. A status/hint line closes it.
pub fn game_config(browser: &Browser) -> Paragraph<'static> {
    let cfg = browser.game_config.as_ref();
    let runner = cfg.map(|c| c.runner.clone()).unwrap_or_else(|| "auto".to_string());
    let proton = cfg
        .and_then(|c| c.forced_proton_version.clone())
        .unwrap_or_else(|| "(default)".to_string());
    let platform = cfg
        .and_then(|c| c.platform_preference.clone())
        .unwrap_or_else(|| "(auto)".to_string());
    let script = match browser.game_script.as_ref() {
        Some(s) if s.exists => s.path.clone().unwrap_or_else(|| "(set)".to_string()),
        _ => "(none)".to_string(),
    };

    let rows: [(&str, String); 4] = [
        ("Runner", runner),
        ("Forced Proton", proton),
        ("Platform", platform),
        ("Launch script", script),
    ];

    let mut lines: Vec<Spans<'static>> = Vec::new();
    for (i, (label, value)) in rows.iter().enumerate() {
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

    lines.push(Spans::from(""));
    let status = if !browser.game_config_status.is_empty() {
        browser.game_config_status.clone()
    } else {
        "[Enter] change  [x] remove launch script  [Esc] close".to_string()
    };
    lines.push(Spans::from(Span::styled(status, theme::dim())));

    paneled_paragraph(lines, "Game settings".to_string())
}
