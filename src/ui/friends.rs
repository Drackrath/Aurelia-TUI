//! The always-visible Friends panel: the logged-in user's Steam friends with
//! their online status and the game they are currently playing.
//!
//! Unlike the old modal overlay, this panel is rendered as a persistent column
//! in the layout. It implements a *whole-list scroll* that keeps the highlighted
//! row (`browser.friends_index`) visible at all times: instead of a fixed
//! selector moving inside a static viewport, the entire list slides so the
//! highlight never falls off the bottom edge.
//!
//! The panel is also *focus-aware*. When `browser.friends_focused` is true the
//! title advertises the chat/window shortcuts and the highlighted row gets the
//! full selection style; when focus is elsewhere the title is calmer and the
//! highlight is softened (marker only, no selection background) so it is obvious
//! that keyboard input is going somewhere else.

use tui::style::Style;
use tui::text::{Span, Spans, Text};
use tui::widgets::Paragraph;

use crate::browse::Browser;
use crate::theme;

/// Build the Friends panel content from the browse state.
///
/// `visible_rows` is the number of CONTENT rows available for friend entries —
/// the caller has already subtracted the block's border, so we render at most
/// `visible_rows` friends and never overflow the panel.
pub fn friends(browser: &Browser, visible_rows: usize) -> Paragraph<'static> {
    let items = &browser.friends;
    let len = items.len();
    let focused = browser.friends_focused;

    // Title: louder and action-hinting while focused, calmer otherwise.
    let title = if focused {
        format!("● Friends ({}) · chat [t] window", len)
    } else {
        format!("Friends ({}) — [F] focus", len)
    };

    // Empty state: no panic, just a dim hint telling the user how to load.
    if items.is_empty() {
        return Paragraph::new(Text::from("Press [F] to load friends."))
            .block(theme::panel(title))
            .style(theme::dim());
    }

    // Whole-list scroll: choose the window start so `friends_index` stays inside
    // the visible window. Once the highlight moves past the bottom edge the whole
    // list shifts up by keeping `friends_index` as the last visible row.
    let start = if visible_rows == 0 {
        0
    } else if browser.friends_index >= visible_rows {
        browser.friends_index + 1 - visible_rows
    } else {
        0
    };

    // Enumerate over the FULL vec first so `i` is the ABSOLUTE friend index, then
    // slice the window with skip/take. This keeps `i == friends_index` correct
    // regardless of how far the list has scrolled.
    let lines: Vec<Spans<'static>> = items
        .iter()
        .enumerate()
        .skip(start)
        .take(visible_rows)
        .map(|(i, f)| {
            let selected = i == browser.friends_index;
            let online = f.is_online();

            // Online dot: green when online, dim otherwise.
            let dot_style = if online {
                Style::default().fg(theme::ONLINE)
            } else {
                theme::dim()
            };

            // Highlight marker: only the selected absolute row gets the arrow.
            let marker = if selected { "▶ " } else { "  " };

            // Name style:
            // - selected AND focused -> full selection highlight.
            // - selected but NOT focused -> soften to plain value (no background)
            //   so it is clear focus is elsewhere; the marker still shows which
            //   row is current.
            // - otherwise -> online = value, offline = dim.
            let name_style = if selected {
                if focused {
                    theme::selection(theme::ACCENT)
                } else {
                    theme::value()
                }
            } else if online {
                theme::value()
            } else {
                theme::dim()
            };

            let mut spans = vec![
                Span::styled(marker, theme::accent()),
                Span::styled("● ", dot_style),
                Span::styled(f.display_name(), name_style),
            ];

            // Append the current game in accent, if any.
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
