//! The wallet overlay (modal popup) showing the Steam Wallet balance.

use tui::layout::Alignment;
use tui::style::{Modifier, Style};
use tui::text::{Span, Spans, Text};
use tui::widgets::Paragraph;

use crate::browse::Browser;
use crate::theme;

/// Build the wallet overlay content from the fetched balance.
pub fn wallet(browser: &Browser) -> Paragraph<'static> {
    let mut lines: Vec<Spans<'static>> = Vec::new();

    match &browser.wallet_info {
        Some(info) => {
            lines.push(Spans::from(""));
            lines.push(Spans::from(Span::styled(
                info.formatted.clone(),
                Style::default()
                    .fg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD),
            )));
            lines.push(Spans::from(""));
            if !info.country.is_empty() {
                lines.push(Spans::from(Span::styled(info.country.clone(), theme::dim())));
            }
        }
        None => {
            lines.push(Spans::from(""));
            lines.push(Spans::from(Span::styled("unavailable", theme::dim())));
        }
    }

    Paragraph::new(Text::from(lines))
        .block(theme::panel("Steam Wallet — press any key to close".to_string()))
        .style(theme::base())
        .alignment(Alignment::Center)
}
