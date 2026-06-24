//! The add-friend overlay (modal popup): a single-line text prompt where the
//! user types a SteamID64 / profile URL / vanity name, runs `friends search` to
//! resolve+preview the account, and confirms the request via `friends add`.
//!
//! The label + input + status lines mirror the shared move/relink/import prompt
//! idiom (see [`crate::ui::prompt_overlay`]); this widget additionally appends a
//! resolved-account preview once a `friends search` has succeeded.

use tui::layout::Alignment;
use tui::style::{Modifier, Style};
use tui::text::{Span, Spans, Text};
use tui::widgets::Paragraph;

use crate::browse::Browser;
use crate::theme;

const TITLE: &str = "Add friend ([Enter] search, [a] send request, [Esc] cancel)";

/// Build the add-friend prompt from the current browser state.
pub fn friend_add_overlay(browser: &Browser) -> Paragraph<'static> {
    // Label + typed query (with caret) + optional status line.
    let mut lines: Vec<Spans<'static>> = vec![
        Spans::from(Span::styled(
            "Friend (SteamID64 / profile URL / vanity):".to_string(),
            Style::default().fg(theme::TEXT),
        )),
        Spans::from(vec![
            Span::styled(browser.friend_add_query.clone(), theme::accent()),
            Span::styled(
                "_".to_string(),
                Style::default()
                    .fg(theme::ACCENT_BRIGHT)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
    ];

    let status = browser
        .friend_add_status
        .lock()
        .map(|s| s.clone())
        .unwrap_or_default();
    if !status.is_empty() {
        lines.push(Spans::from(""));
        lines.push(Spans::from(Span::styled(
            status,
            Style::default().fg(theme::WARN).add_modifier(Modifier::DIM),
        )));
    }

    // Resolved-account preview, filled in after a successful `friends search`.
    if let Some(found) = &browser.friend_search_result {
        lines.push(Spans::from(""));
        lines.push(Spans::from(vec![
            Span::styled("Resolved: ", Style::default().fg(theme::TEXT_DIM)),
            Span::styled(
                found.display_name(),
                Style::default()
                    .fg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  ({})", found.steam_id),
                Style::default().fg(theme::TEXT_DIM),
            ),
        ]));
        lines.push(Spans::from(Span::styled(
            "Press [a] to send the friend request.".to_string(),
            Style::default().fg(theme::TEXT_DIM),
        )));
    }

    Paragraph::new(Text::from(lines))
        .block(theme::panel(TITLE.to_string()))
        .style(theme::base())
        .alignment(Alignment::Left)
}
