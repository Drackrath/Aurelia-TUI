//! The uninstall confirmation overlay (modal popup).

use tui::layout::Alignment;
use tui::style::{Modifier, Style};
use tui::text::{Span, Spans, Text};
use tui::widgets::{BorderType, Borders, Paragraph};

use crate::theme;

/// Build the uninstall confirmation prompt for `game_name`.
pub fn confirm_uninstall(game_name: &str) -> Paragraph<'static> {
    confirm(format!("Uninstall {}?", game_name))
}

/// Build the remove-friend confirmation prompt for `friend_name`.
pub fn confirm_remove_friend(friend_name: &str) -> Paragraph<'static> {
    confirm(format!("Remove {}?", friend_name))
}

/// Shared destructive-action confirmation popup: a `prompt` question over a
/// `[y] confirm / [n] cancel` hint, framed by a WARN-accented border/title.
fn confirm(prompt: String) -> Paragraph<'static> {
    let lines = vec![
        Spans::from(Span::styled(prompt, Style::default().fg(theme::TEXT))),
        Spans::from(""),
        Spans::from(vec![
            Span::styled("[y]", theme::key()),
            Span::styled(" confirm   ", Style::default().fg(theme::TEXT_DIM)),
            Span::styled("[n]", theme::key()),
            Span::styled(" cancel", Style::default().fg(theme::TEXT_DIM)),
        ]),
    ];

    // A panel styled like [`theme::panel`] but with a WARN-accented border/title
    // to signal the destructive action.
    let block = tui::widgets::Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::WARN).bg(theme::BG))
        .title(Spans::from(Span::styled(
            "Confirm".to_string(),
            Style::default()
                .fg(theme::WARN)
                .bg(theme::BG)
                .add_modifier(Modifier::BOLD),
        )))
        .style(theme::base());

    Paragraph::new(Text::from(lines))
        .block(block)
        .style(theme::base())
        .alignment(Alignment::Center)
}
