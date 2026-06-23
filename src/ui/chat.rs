//! The chat view (modal popup): the recent message history with a friend, plus
//! an input line for composing a new message. Opened from the friends overlay.

use tui::layout::Alignment;
use tui::style::{Modifier, Style};
use tui::text::{Span, Spans, Text};
use tui::widgets::Paragraph;

use crate::browse::Browser;
use crate::theme;

/// Build the chat view content from the current browser state: the message
/// history (sender label + text per message), a separator, and the composition
/// input line with a trailing caret.
pub fn chat(browser: &Browser) -> Paragraph<'static> {
    let title = format!("Chat — {} ([Esc] close)", browser.chat_partner);

    let mut lines: Vec<Spans<'static>> = Vec::new();

    if browser.chat_messages.is_empty() {
        lines.push(Spans::from(Span::styled(
            "No messages yet.".to_string(),
            theme::dim(),
        )));
    } else {
        for m in browser.chat_messages.iter().skip(browser.chat_scroll) {
            let (label, label_style) = if m.from_self {
                ("me: ".to_string(), theme::dim())
            } else {
                (format!("{}: ", browser.chat_partner), theme::accent())
            };
            lines.push(Spans::from(vec![
                Span::styled(label, label_style),
                Span::styled(m.message.clone(), theme::value()),
            ]));
        }
    }

    // Separator, then the composition input line.
    lines.push(Spans::from(Span::styled(
        "─".repeat(40),
        Style::default().fg(theme::BORDER),
    )));
    lines.push(Spans::from(vec![
        Span::styled("> ".to_string(), theme::accent()),
        Span::styled(browser.chat_input.clone(), theme::value()),
        Span::styled(
            "_".to_string(),
            Style::default()
                .fg(theme::ACCENT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    Paragraph::new(Text::from(lines))
        .block(theme::panel(title))
        .style(theme::base())
        .alignment(Alignment::Left)
}
