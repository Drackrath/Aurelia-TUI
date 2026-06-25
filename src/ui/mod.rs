//! Browse-view widgets. Each submodule is a pure builder that renders from the
//! [`crate::browse::Browser`] state plus the shared [`crate::theme`], so the
//! event loop only has to lay them out.

pub mod chat;
pub mod cloud;
pub mod achievements;
pub mod account;
pub mod branches;
pub mod config;
pub mod confirm;
pub mod depots;
pub mod detail;
pub mod dlc;
pub mod friend_add;
pub mod friends;
pub mod help;
pub mod import;
pub mod inventory;
pub mod launch;
pub mod list;
pub mod market;
pub mod market_search;
pub mod move_game;
pub mod proton;
pub mod relink;
pub mod running;
pub mod status;
pub mod tabs;
pub mod wallet;
pub mod workshop;

use tui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use tui::style::{Modifier, Style};
use tui::text::{Span, Spans, Text};
use tui::widgets::{ListItem, Paragraph};

use crate::theme;

/// A [`Paragraph`] framed by the standard [`theme::panel`] (titled, soft-blue
/// border) over a [`theme::base`] fill — the common scaffolding behind the
/// scrollable list overlays (achievements, depots, inventory, market, launch …).
///
/// `content` is anything convertible into a [`Text`] — a `Vec<Spans>` of rows, a
/// single styled `Span`, or a plain `&str` empty-state line — so each builder
/// hands over the body it assembled and shares the block/style wiring.
pub fn paneled_paragraph(content: impl Into<Text<'static>>, title: String) -> Paragraph<'static> {
    Paragraph::new(content)
        .block(theme::panel(title))
        .style(theme::base())
}

/// A shared single-line text-entry overlay: a `label`, the typed `value` (with a
/// trailing caret), an optional `status` line, all framed by `title`.
///
/// This is the common builder behind the move/relink/import path prompts and the
/// friend search/add prompt — a pure builder from its arguments (no `Browser`
/// coupling) so each caller passes the fields it stores.
pub fn prompt_overlay(label: &str, value: &str, status: &str, title: &str) -> Paragraph<'static> {
    let mut lines: Vec<Spans<'static>> = vec![
        Spans::from(Span::styled(
            label.to_string(),
            Style::default().fg(theme::TEXT),
        )),
        Spans::from(vec![
            Span::styled(value.to_string(), theme::accent()),
            Span::styled(
                "_".to_string(),
                Style::default()
                    .fg(theme::ACCENT_BRIGHT)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
    ];

    if !status.is_empty() {
        lines.push(Spans::from(""));
        lines.push(Spans::from(Span::styled(
            status.to_string(),
            Style::default().fg(theme::WARN).add_modifier(Modifier::DIM),
        )));
    }

    Paragraph::new(Text::from(lines))
        .block(theme::panel(title.to_string()))
        .style(theme::base())
        .alignment(Alignment::Left)
}

/// The leading marker for a row in a selectable list overlay: the `▶ ` selection
/// glyph for the highlighted row, or two spaces of padding otherwise so every
/// row stays column-aligned. Centralizes the marker glyph shared by the
/// branches/dlc/proton/running/friends/market-search lists; callers still own the
/// per-list name styling, since that varies (online/installed/focus-aware).
pub fn selection_marker(selected: bool) -> &'static str {
    if selected {
        "▶ "
    } else {
        "  "
    }
}

/// The single dimmed placeholder row a list overlay shows when it has no
/// entries (e.g. "No DLC.", "No branches."). Returns a one-element `Vec` so it
/// drops straight into the `items` a [`tui::widgets::List`] is built from.
pub fn empty_list_rows(message: impl Into<String>) -> Vec<ListItem<'static>> {
    vec![ListItem::new(Spans::from(Span::styled(
        message.into(),
        theme::dim(),
    )))]
}

/// A rectangle centered within `area`, sized as a percentage of it. Used to
/// place modal overlays (e.g. the help popup).
pub fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}
