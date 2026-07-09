//! The collections overlay (modal popup): the account's Steam library
//! collections, with the highlighted row for adding/removing the current game,
//! plus create/delete and cloud pull/push/sync. Shows an inline name prompt
//! while creating.

use tui::text::{Span, Spans};
use tui::widgets::Paragraph;

use crate::browse::Browser;
use crate::theme;
use crate::ui::{paneled_paragraph, selection_marker};

/// Build the collections overlay from the browser's open collections state. In
/// create mode it shows the name prompt; otherwise a selectable list of
/// collections (each with a game count and a dynamic marker) and a status/hint
/// line.
pub fn collections(browser: &Browser) -> Paragraph<'static> {
    let mut lines: Vec<Spans<'static>> = Vec::new();

    if let Some(input) = &browser.collections_input {
        lines.push(Spans::from(vec![
            Span::styled("New collection name: ".to_string(), theme::label()),
            Span::styled(input.clone(), theme::accent()),
            Span::styled("_".to_string(), theme::key()),
        ]));
        lines.push(Spans::from(""));
        lines.push(Spans::from(Span::styled(
            "Enter to create · Esc to cancel".to_string(),
            theme::dim(),
        )));
        return paneled_paragraph(lines, "Collections".to_string());
    }

    if browser.collections.is_empty() {
        lines.push(Spans::from(Span::styled(
            "No collections. [n] to create one.".to_string(),
            theme::dim(),
        )));
    } else {
        for (i, c) in browser.collections.iter().enumerate() {
            let selected = i == browser.collections_index;
            let style = if selected {
                theme::selection(theme::ACCENT)
            } else {
                theme::value()
            };
            let count = c
                .count
                .map(|n| n.to_string())
                .unwrap_or_else(|| c.app_ids.len().to_string());
            let kind = if c.dynamic { " · dynamic" } else { "" };
            lines.push(Spans::from(vec![
                Span::styled(selection_marker(selected).to_string(), style),
                Span::styled(c.name.clone(), style),
                Span::raw("  ".to_string()),
                Span::styled(format!("{count} games{kind}"), theme::dim()),
            ]));
        }
    }

    lines.push(Spans::from(""));
    let status = if !browser.collections_status.is_empty() {
        browser.collections_status.clone()
    } else {
        "[a] add game  [r] remove game  [n] new  [x] delete  [P] pull [U] push [S] sync  [Esc] close"
            .to_string()
    };
    lines.push(Spans::from(Span::styled(status, theme::dim())));

    paneled_paragraph(lines, "Collections".to_string())
}
