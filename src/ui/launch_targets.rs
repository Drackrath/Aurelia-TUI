//! The launch-targets overlay ("Play as…", modal popup): every way an installed
//! game can be started — the native Linux build, or the Windows build under each
//! Steam mode — from `aurelia launch-targets`. Enter launches the highlighted one
//! for this run only, without changing the saved per-game config.

use tui::text::{Span, Spans};
use tui::widgets::Paragraph;

use crate::browse::Browser;
use crate::theme;
use crate::ui::{paneled_paragraph, selection_marker};

/// Build the launch-targets overlay from the browser's open state. Each row is a
/// target label, tagged `default` (what plain `play` picks) or with what is
/// missing when it is unavailable. A status/hint line closes it.
pub fn launch_targets(browser: &Browser) -> Paragraph<'static> {
    let mut lines: Vec<Spans<'static>> = Vec::new();

    if browser.launch_targets.is_empty() {
        lines.push(Spans::from(Span::styled(
            "No launch targets.".to_string(),
            theme::dim(),
        )));
    }

    for (i, target) in browser.launch_targets.iter().enumerate() {
        let selected = i == browser.launch_targets_index;
        let style = if selected {
            theme::selection(theme::ACCENT)
        } else if target.available {
            theme::value()
        } else {
            theme::dim()
        };
        let mut spans = vec![
            Span::styled(selection_marker(selected).to_string(), style),
            Span::styled(target.label(), style),
        ];
        if browser.launch.saved.as_deref() == Some(target.id.as_str()) {
            spans.push(Span::styled("  [saved]".to_string(), theme::accent()));
        }
        if target.default {
            spans.push(Span::styled("  [default]".to_string(), theme::accent()));
        }
        if !target.available {
            let missing = target.missing.as_deref().unwrap_or("unavailable");
            spans.push(Span::styled(format!("  (missing: {missing})"), theme::dim()));
        }
        lines.push(Spans::from(spans));
    }

    lines.push(Spans::from(""));
    lines.push(Spans::from(Span::styled(play_uses(browser), theme::label())));
    if let Some(progress) = browser.launch.runtime.progress_text() {
        lines.push(Spans::from(Span::styled(progress, theme::accent())));
    }
    if !browser.launch_targets_status.is_empty() {
        lines.push(Spans::from(Span::styled(
            browser.launch_targets_status.clone(),
            theme::dim(),
        )));
    }
    lines.push(Spans::from(Span::styled(hints(browser), theme::dim())));

    paneled_paragraph(lines, "Play as…".to_string())
}

/// Which target plain Play uses.
fn play_uses(browser: &Browser) -> String {
    match browser.launch.saved.as_deref() {
        None => "Play uses: automatic".to_string(),
        Some(id) => match browser.launch_targets.iter().find(|t| t.id == id) {
            Some(t) if t.available => format!("Play uses: {} (saved)", t.label()),
            _ => format!("Play uses: default ({id} unavailable)"),
        },
    }
}

/// Key hints for the highlighted row.
fn hints(browser: &Browser) -> String {
    let mut hints = String::from("[Enter] launch once  [d] set as default  [a] automatic");
    if browser.launch_target_needs_runtime() && !browser.launch.runtime.is_running() {
        hints.push_str("  [i] install Steam runtime");
    }
    hints.push_str("  [Esc] close");
    hints
}
