//! The per-game Actions menu (modal command palette): every action applicable
//! to the selected game, grouped by category, with a live text filter and the
//! base-keymap accelerator shown next to each row. This is the primary way to
//! reach the many per-game actions without memorising the hotkeys.

use tui::text::{Span, Spans};
use tui::widgets::Paragraph;

use crate::browse::Browser;
use crate::theme;
use crate::ui::scroll::window;
use crate::ui::{paneled_paragraph, selection_marker};

/// Actions menu, unclipped.
pub fn actions(browser: &Browser) -> Paragraph<'static> {
    actions_in(browser, u16::MAX)
}

/// Actions menu for a `height`-row popup.
pub fn actions_in(browser: &Browser, height: u16) -> Paragraph<'static> {
    let rows = browser.filtered_actions();
    let mut body: Vec<Spans<'static>> = Vec::new();
    let mut is_header: Vec<bool> = Vec::new();
    let mut selected_line = 0;
    let mut current_category = "";
    for (i, row) in rows.iter().enumerate() {
        if row.category != current_category {
            current_category = row.category;
            body.push(Spans::from(Span::styled(
                current_category.to_string(),
                theme::title_style(),
            )));
            is_header.push(true);
        }
        let selected = i == browser.actions_index;
        if selected {
            selected_line = body.len();
        }
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
        body.push(Spans::from(spans));
        is_header.push(false);
    }

    // Border plus hint and blank.
    let room = usize::from(height).saturating_sub(4).max(2);
    let shown = window_lines(body, &is_header, selected_line, room);

    let position = if rows.is_empty() {
        String::new()
    } else {
        format!(" ({}/{})", browser.actions_index + 1, rows.len())
    };
    let header = if browser.actions_filter.is_empty() {
        "↑↓ move · Enter run · Esc close · type to filter".to_string()
    } else {
        format!("filter: {}", browser.actions_filter)
    };
    let mut lines = vec![
        Spans::from(Span::styled(header, theme::dim())),
        Spans::from(""),
    ];
    if rows.is_empty() {
        lines.push(Spans::from(Span::styled(
            "No matching actions.".to_string(),
            theme::dim(),
        )));
    }
    lines.extend(shown);

    let title = match browser.selected() {
        Some(game) => format!("Actions{position} — {}", game.name),
        None => format!("Actions{position}"),
    };
    paneled_paragraph(lines, title)
}

/// Visible lines, keeping the group header.
fn window_lines(
    body: Vec<Spans<'static>>,
    is_header: &[bool],
    selected: usize,
    room: usize,
) -> Vec<Spans<'static>> {
    if body.len() <= room {
        return body;
    }
    // One line kept for sticky header.
    let (start, end) = window(selected, body.len(), room - 1);
    if is_header[start] {
        let end = (end + 1).min(body.len());
        return body[start..end].to_vec();
    }
    let group = (0..start).rev().find(|&i| is_header[i]);
    let mut out = Vec::with_capacity(room);
    if let Some(g) = group {
        out.push(body[g].clone());
    }
    out.extend(body[start..end].iter().cloned());
    out
}
