//! The friends overlay (modal popup): the logged-in user's Steam friends with
//! their online status and current game, scrollable.

use tui::style::Style;
use tui::text::{Span, Spans, Text};
use tui::widgets::Paragraph;

use crate::browse::Browser;
use crate::theme;

/// Build the friends overlay content from the browse state. Each row is an
/// online dot (green online / dim offline), the friend's name, and the game
/// they are currently playing (if any). The list is scrolled by
/// `browser.friends_scroll`.
pub fn friends(browser: &Browser) -> Paragraph<'static> {
    let items = &browser.friends;
    let title = format!("Friends ({})", items.len());

    if items.is_empty() {
        return Paragraph::new(Text::from("No friends."))
            .block(theme::panel(title))
            .style(theme::base());
    }

    let lines: Vec<Spans<'static>> = items
        .iter()
        .enumerate()
        .skip(browser.friends_scroll)
        .map(|(i, f)| {
            let selected = i == browser.friends_index;
            let online = f.is_online();
            let dot_style = if online {
                Style::default().fg(theme::ONLINE)
            } else {
                theme::dim()
            };
            let name_style = if selected {
                theme::selection(theme::ACCENT)
            } else if online {
                theme::value()
            } else {
                theme::dim()
            };
            let marker = if selected { "▶ " } else { "  " };
            let mut spans = vec![
                Span::styled(marker, theme::accent()),
                Span::styled("● ", dot_style),
                Span::styled(f.display_name(), name_style),
            ];
            if let Some(game) = f.current_game() {
                spans.push(Span::styled(format!(" — {}", game), theme::accent()));
            }
            Spans::from(spans)
        })
        .collect();

    Paragraph::new(Text::from(lines))
        .block(theme::panel(title))
        .style(theme::base())
}
