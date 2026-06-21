//! The help overlay (modal popup) listing every key binding.

use tui::layout::Alignment;
use tui::style::{Modifier, Style};
use tui::text::{Span, Spans, Text};
use tui::widgets::Paragraph;

use crate::theme;

/// A bold accent group header.
fn header(title: &str) -> Spans<'static> {
    Spans::from(Span::styled(
        title.to_string(),
        Style::default()
            .fg(theme::ACCENT)
            .add_modifier(Modifier::BOLD),
    ))
}

/// A "key — description" binding line.
fn binding(keys: &str, desc: &str) -> Spans<'static> {
    Spans::from(vec![
        Span::styled(format!("  {:<14}", keys), theme::key()),
        Span::styled(desc.to_string(), Style::default().fg(theme::TEXT)),
    ])
}

/// Build the help overlay content.
pub fn help() -> Paragraph<'static> {
    let mut lines: Vec<Spans<'static>> = Vec::new();

    lines.push(header("Navigation"));
    lines.push(binding("j / ↓", "move down"));
    lines.push(binding("k / ↑", "move up"));
    lines.push(binding("g", "jump to top"));
    lines.push(binding("G", "jump to bottom"));
    lines.push(binding("PageUp/PageDown", "page up / down"));
    lines.push(binding("mouse wheel", "scroll the list"));
    lines.push(Spans::from(""));

    lines.push(header("Filtering"));
    lines.push(binding("Tab / Shift-Tab", "cycle view"));
    lines.push(binding("1 – 4", "jump to a view"));
    lines.push(binding("/", "focus text filter"));
    lines.push(binding("Esc", "clear filter"));
    lines.push(binding("s", "cycle sort"));
    lines.push(Spans::from(""));

    lines.push(header("Actions"));
    lines.push(binding("Enter", "launch game"));
    lines.push(binding("i", "expand / collapse description"));
    lines.push(binding("d", "install / download"));
    lines.push(binding("v", "verify files"));
    lines.push(binding("f", "toggle favourite"));
    lines.push(binding("H", "hide game"));
    lines.push(binding("r", "refresh library"));
    lines.push(binding("l", "sign in again"));
    lines.push(Spans::from(""));

    lines.push(header("General"));
    lines.push(binding("?", "toggle this help"));
    lines.push(binding("q", "quit"));

    let text = Text::from(lines);

    Paragraph::new(text)
        .block(theme::panel("Help — press any key to close".to_string()))
        .style(theme::base())
        .alignment(Alignment::Left)
}
