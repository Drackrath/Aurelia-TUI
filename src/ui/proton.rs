//! The Proton/Wine runtime overlay (modal popup): the available runtimes with
//! their installed/default state, and the highlighted row for set-default.

use tui::layout::Rect;
use tui::style::Modifier;
use tui::text::{Span, Spans};
use tui::widgets::Paragraph;

use crate::browse::Browser;
use crate::theme;
use crate::ui::play_settings::{tail_fit, wrap_hints};

/// Configured default, and its resolved row.
fn default_line(browser: &Browser) -> Option<String> {
    let default = &browser.play.proton_default;
    if default.is_empty() {
        return None;
    }
    Some(match browser.play.proton_default_row.as_deref() {
        Some(row) if !row.eq_ignore_ascii_case(default) => {
            format!("Default: {default} → {row}")
        }
        Some(_) => format!("Default: {default}"),
        None => format!("Default: {default} (not installed)"),
    })
}

/// Proton overlay sized to `area`.
///
/// Rows scroll; keys sit below.
pub fn proton(browser: &Browser, area: Rect) -> Paragraph<'static> {
    let inner_w = area.width.saturating_sub(2) as usize;
    let inner_h = area.height.saturating_sub(2) as usize;

    let mut header: Vec<Spans<'static>> = Vec::new();
    if let Some(line) = default_line(browser) {
        header.push(Spans::from(Span::styled(
            tail_fit(&line, inner_w),
            theme::value().fg(theme::ACCENT),
        )));
    }

    let mut footer: Vec<Spans<'static>> = Vec::new();
    if let Some(status) = browser.proton_status_line() {
        footer.push(Spans::from(Span::styled(
            tail_fit(&status, inner_w),
            theme::dim(),
        )));
    }
    let keys = [
        "[d] default",
        "[i] install",
        "[u] remove",
        "[e] EAC",
        "[b] BattlEye",
        "[Esc] close",
    ];
    for h in wrap_hints(&keys, inner_w) {
        footer.push(Spans::from(Span::styled(h, theme::dim())));
    }

    let rows_h = inner_h.saturating_sub(header.len() + footer.len());
    let mut lines = header;
    if browser.protons.is_empty() {
        lines.push(Spans::from(Span::styled(
            "No runtimes.".to_string(),
            theme::dim(),
        )));
    } else {
        let (start, end) =
            crate::ui::scroll::window(browser.proton_index, browser.protons.len(), rows_h);
        for (i, entry) in browser.protons.iter().enumerate().take(end).skip(start) {
            let selected = i == browser.proton_index;
            let name_style = if selected {
                theme::selection(theme::ACCENT)
            } else if entry.installed {
                theme::value()
            } else {
                theme::dim()
            };
            let mut spans = vec![
                Span::styled(
                    crate::ui::selection_marker(selected).to_string(),
                    name_style,
                ),
                Span::styled(entry.name.clone(), name_style),
            ];
            // Default (accent) or installed (green).
            if entry.is_default {
                spans.push(Span::raw("  ".to_string()));
                spans.push(Span::styled(
                    "[default]".to_string(),
                    theme::value()
                        .fg(theme::ACCENT)
                        .add_modifier(Modifier::BOLD),
                ));
            } else if entry.installed {
                spans.push(Span::raw("  ".to_string()));
                spans.push(Span::styled(
                    "[installed]".to_string(),
                    theme::value().fg(theme::ONLINE),
                ));
            }
            lines.push(Spans::from(spans));
        }
    }
    // Pin the footer to the bottom.
    while lines.len() + footer.len() < inner_h {
        lines.push(Spans::from(""));
    }
    lines.extend(footer);

    let title = format!("Proton runtimes ({})", browser.protons.len());
    crate::ui::paneled_paragraph(lines, title)
}
