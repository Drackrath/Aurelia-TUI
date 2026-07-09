//! The per-game Actions menu (modal command palette): every action applicable
//! to the selected game, grouped by category, with a live text filter and the
//! base-keymap accelerator shown next to each row. This is the primary way to
//! reach the many per-game actions without memorising the hotkeys.

use tui::text::{Span, Spans};
use tui::widgets::Paragraph;

use crate::browse::Browser;
use crate::theme;
use crate::ui::{paneled_paragraph, selection_marker};

/// Build the Actions menu from the browser's open action state. Rows are the
/// filtered, applicable actions grouped under category headers; the highlighted
/// row (`actions_index` into the filtered list) gets a selection bar. A filter
/// line at the top echoes what the user has typed.
pub fn actions(browser: &Browser) -> Paragraph<'static> {
    let rows = browser.filtered_actions();
    let mut lines: Vec<Spans<'static>> = Vec::new();

    // Filter / hint line.
    let header = if browser.actions_filter.is_empty() {
        "type to filter · ↑↓ move · Enter select · Esc close".to_string()
    } else {
        format!("filter: {}", browser.actions_filter)
    };
    lines.push(Spans::from(Span::styled(header, theme::dim())));
    lines.push(Spans::from(""));

    if rows.is_empty() {
        lines.push(Spans::from(Span::styled(
            "No matching actions.".to_string(),
            theme::dim(),
        )));
    } else {
        let mut current_category = "";
        for (i, row) in rows.iter().enumerate() {
            if row.category != current_category {
                current_category = row.category;
                lines.push(Spans::from(Span::styled(
                    current_category.to_string(),
                    theme::title_style(),
                )));
            }
            let selected = i == browser.actions_index;
            let style = if selected {
                theme::selection(theme::ACCENT)
            } else {
                theme::value()
            };
            let mut spans = vec![
                Span::styled(selection_marker(selected).to_string(), style),
                Span::styled(row.label.to_string(), style),
            ];
            if !row.key.is_empty() {
                spans.push(Span::raw("  ".to_string()));
                spans.push(Span::styled(format!("[{}]", row.key), theme::dim()));
            }
            lines.push(Spans::from(spans));
        }
    }

    let title = match browser.selected() {
        Some(game) => format!("Actions — {}", game.name),
        None => "Actions".to_string(),
    };
    paneled_paragraph(lines, title)
}
