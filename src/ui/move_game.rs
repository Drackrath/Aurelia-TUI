//! The move (relocate install) overlay (modal popup): a text prompt where the
//! user types the destination Steam library folder. (`move` is a Rust keyword,
//! hence the module name `move_game`.)

use tui::layout::Alignment;
use tui::style::{Modifier, Style};
use tui::text::{Span, Spans, Text};
use tui::widgets::Paragraph;

use crate::browse::Browser;
use crate::theme;

/// Build the move prompt content from the current browser state: a label, the
/// typed destination path (with a trailing caret), and the status line if any.
pub fn move_overlay(browser: &Browser) -> Paragraph<'static> {
    let mut lines: Vec<Spans<'static>> = vec![
        Spans::from(Span::styled(
            "Destination library folder:".to_string(),
            Style::default().fg(theme::TEXT),
        )),
        Spans::from(vec![
            Span::styled(browser.move_path.clone(), theme::accent()),
            Span::styled(
                "_".to_string(),
                Style::default()
                    .fg(theme::ACCENT_BRIGHT)
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
    ];

    if !browser.move_status.is_empty() {
        lines.push(Spans::from(""));
        lines.push(Spans::from(Span::styled(
            browser.move_status.clone(),
            Style::default().fg(theme::WARN).add_modifier(Modifier::DIM),
        )));
    }

    Paragraph::new(Text::from(lines))
        .block(theme::panel(
            "Move install ([Enter] confirm, [Esc] cancel)".to_string(),
        ))
        .style(theme::base())
        .alignment(Alignment::Left)
}
