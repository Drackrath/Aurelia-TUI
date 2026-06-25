//! The market overlay (modal popup): the logged-in user's active Community
//! Market listings and open buy orders, scrollable.

use tui::text::{Span, Spans};
use tui::widgets::Paragraph;

use crate::browse::Browser;
use crate::theme;
use crate::ui::paneled_paragraph;

/// Build the market overlay content from the browse state. Each row is the
/// item's market hash name, its price, and a dim kind tag (listing / buy
/// order). The list is scrolled by `browser.market_scroll`.
pub fn market(browser: &Browser) -> Paragraph<'static> {
    let items = &browser.market;
    let title = format!("Market listings ({})", items.len());

    if items.is_empty() {
        return paneled_paragraph("No active listings.", title);
    }

    let lines: Vec<Spans<'static>> = items
        .iter()
        .skip(browser.market_scroll)
        .map(|m| {
            let mut spans = vec![
                Span::styled(m.name.clone(), theme::value()),
                Span::styled(format!("  {}", m.price_text()), theme::accent()),
            ];
            if m.quantity > 1 {
                spans.push(Span::styled(format!(" x{}", m.quantity), theme::dim()));
            }
            spans.push(Span::styled(format!("  [{}]", m.kind), theme::dim()));
            Spans::from(spans)
        })
        .collect();

    paneled_paragraph(lines, title)
}
