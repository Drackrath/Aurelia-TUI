//! The Workshop overlay (modal popup): the selected game's subscribed/installed
//! Workshop items, scrollable.

use pretty_bytes::converter::convert;

use tui::style::Style;
use tui::text::{Span, Spans, Text};
use tui::widgets::Paragraph;

use crate::browse::Browser;
use crate::theme;

/// Build the Workshop overlay content from the browse state. Each row is a state
/// marker (● installed / ○ subscribed-only), the item title, and its size when
/// known. The list is scrolled by `browser.workshop_scroll`.
pub fn workshop(browser: &Browser) -> Paragraph<'static> {
    let items = &browser.workshop;
    let title = format!("Workshop ({})", items.len());

    if items.is_empty() {
        return Paragraph::new(Text::from("No workshop items."))
            .block(theme::panel(title))
            .style(theme::base());
    }

    let lines: Vec<Spans<'static>> = items
        .iter()
        .skip(browser.workshop_scroll)
        .map(|item| {
            let (glyph, glyph_style) = if item.installed {
                ("● ", Style::default().fg(theme::ONLINE))
            } else {
                ("○ ", theme::dim())
            };
            let mut spans = vec![
                Span::styled(glyph, glyph_style),
                Span::styled(item.display_title(), theme::value()),
            ];
            if item.file_size > 0 {
                spans.push(Span::styled(
                    format!(" ({})", convert(item.file_size as f64)),
                    theme::dim(),
                ));
            }
            Spans::from(spans)
        })
        .collect();

    Paragraph::new(Text::from(lines))
        .block(theme::panel(title))
        .style(theme::base())
}
