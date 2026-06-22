//! The depots overlay (modal popup): the selected game's depots, each with its
//! id/name and human-readable size. Read-only; j/k scroll.

use pretty_bytes::converter::convert;

use tui::text::{Span, Spans, Text};
use tui::widgets::Paragraph;

use crate::browse::Browser;
use crate::theme;

/// Build the depots overlay from the browser's open depots state. Each row shows
/// the depot name (falling back to its id) and the (human-readable) size; the
/// `depots_scroll` offset selects the first visible row. Renders a single "No
/// depots." line when empty.
pub fn depots(browser: &Browser) -> Paragraph<'static> {
    let lines: Vec<Spans<'static>> = if browser.depots.is_empty() {
        vec![Spans::from(Span::styled("No depots.".to_string(), theme::dim()))]
    } else {
        browser
            .depots
            .iter()
            .skip(browser.depots_scroll)
            .map(|depot| {
                let label = if depot.name.is_empty() {
                    depot.id.to_string()
                } else {
                    format!("{} ({})", depot.name, depot.id)
                };
                Spans::from(vec![
                    Span::styled(label, theme::value()),
                    Span::raw("  ".to_string()),
                    Span::styled(convert(depot.size as f64), theme::dim()),
                ])
            })
            .collect()
    };

    Paragraph::new(Text::from(lines))
        .block(theme::panel(format!("Depots ({})", browser.depots.len())))
        .style(theme::base())
}
