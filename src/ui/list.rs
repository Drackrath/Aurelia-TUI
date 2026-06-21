//! The game list with status badges.

use tui::text::{Span, Spans};
use tui::widgets::{List, ListItem};

use crate::browse::{badge, Browser};
use crate::theme;
use crate::util::stateful::Named;

/// Build the game list for the current view (badges + names).
pub fn list(browser: &Browser) -> List<'static> {
    let items: Vec<ListItem<'static>> = browser
        .visible()
        .iter()
        .map(|game| {
            let b = badge(game);

            // Pick the name style: muted when not installed ("○"), otherwise
            // installed; updates/failures override based on the badge note.
            let name_style = if b.note.as_deref() == Some("update") {
                theme::item_update()
            } else if b.note.as_deref() == Some("failed") {
                theme::item_failed()
            } else if b.glyph == "○" {
                theme::item_muted()
            } else {
                theme::item_installed()
            };

            let mut spans: Vec<Span<'static>> = vec![
                Span::styled(b.glyph.to_string(), b.style),
                Span::raw(" ".to_string()),
                Span::styled(game.get_name(), name_style),
            ];

            if let Some(note) = b.note {
                spans.push(Span::styled(format!("  {}", note), theme::dim()));
            }

            ListItem::new(Spans::from(spans))
        })
        .collect();

    List::new(items)
        .block(theme::panel(format!(
            "{} ({})",
            browser.filter.label(),
            browser.visible_len()
        )))
        .highlight_style(theme::selection(theme::ACCENT))
        .highlight_symbol("▶ ")
}
