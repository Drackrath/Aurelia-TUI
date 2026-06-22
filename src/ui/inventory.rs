//! The inventory overlay (modal popup): the logged-in user's Steam inventory
//! for the selected game, scrollable.

use tui::text::{Span, Spans, Text};
use tui::widgets::Paragraph;

use crate::browse::Browser;
use crate::theme;

/// Build the inventory overlay content from the browse state. Each row is the
/// item name, its stack count (accented when more than one), and a dim type
/// tag. The list is scrolled by `browser.inv_scroll`.
pub fn inventory(browser: &Browser) -> Paragraph<'static> {
    let items = &browser.inventory;
    let title = format!("Inventory ({})", items.len());

    if items.is_empty() {
        return Paragraph::new(Text::from("No inventory items."))
            .block(theme::panel(title))
            .style(theme::base());
    }

    let lines: Vec<Spans<'static>> = items
        .iter()
        .skip(browser.inv_scroll)
        .map(|item| {
            let mut spans = vec![Span::styled(item.display_name(), theme::value())];
            if item.amount > 1 {
                spans.push(Span::styled(format!(" x{}", item.amount), theme::accent()));
            }
            if !item.item_type.is_empty() {
                spans.push(Span::styled(format!(" ({})", item.item_type), theme::dim()));
            }
            Spans::from(spans)
        })
        .collect();

    Paragraph::new(Text::from(lines))
        .block(theme::panel(title))
        .style(theme::base())
}
