//! Shared y/n confirmation popups, sized to text.

use tui::Frame;
use tui::backend::Backend;
use tui::layout::{Alignment, Rect};
use tui::style::{Modifier, Style};
use tui::text::{Span, Spans, Text};
use tui::widgets::{BorderType, Borders, Clear, Paragraph, Wrap};

use crate::theme;

/// Uninstall question for `name`.
pub fn uninstall_text(name: &str) -> String {
    format!("Uninstall {name}?")
}

/// Remove-friend question for `name`.
pub fn remove_friend_text(name: &str) -> String {
    format!("Remove {name}?")
}

/// Reconnect question; it logs on again.
pub fn reconnect_text() -> String {
    "Reconnect Steam? (new logon)".to_string()
}

/// Install-now question for `name`.
pub fn install_text(name: &str) -> String {
    format!("{name} is not installed. Install now?")
}

/// Build the uninstall confirmation prompt for `game_name`.
pub fn confirm_uninstall(game_name: &str) -> Paragraph<'static> {
    confirm(uninstall_text(game_name))
}

/// Build the remove-friend confirmation prompt for `friend_name`.
pub fn confirm_remove_friend(friend_name: &str) -> Paragraph<'static> {
    confirm(remove_friend_text(friend_name))
}

/// Reconnect prompt; reconnecting logs on again.
pub fn confirm_reconnect() -> Paragraph<'static> {
    confirm(reconnect_text())
}

/// Build the install-now prompt for `game_name`.
pub fn confirm_install(game_name: &str) -> Paragraph<'static> {
    confirm(install_text(game_name))
}

/// Hint line text, for sizing.
const HINT: &str = "[y] confirm   [n] cancel";
/// Smallest inner width.
const MIN_INNER: usize = 30;

/// Greedy word-wrap line count.
fn wrapped_lines(text: &str, width: usize) -> usize {
    let width = width.max(1);
    let mut lines = 1;
    let mut used = 0;
    for word in text.split_whitespace() {
        let len = word.chars().count();
        let need = if used == 0 { len } else { used + 1 + len };
        if need <= width {
            used = need;
        } else {
            lines += 1 + (len.saturating_sub(1)) / width;
            used = len % width;
            if used == 0 {
                used = width;
            }
        }
    }
    lines
}

/// Centered rect fitting `prompt` plus hint.
pub fn popup_area(frame: Rect, prompt: &str) -> Rect {
    let max_inner = usize::from(frame.width.saturating_sub(4)).max(1);
    let want = prompt.chars().count().max(HINT.len()).max(MIN_INNER);
    let inner = want.min(max_inner);
    // Prompt, blank, hint, borders.
    let height = (wrapped_lines(prompt, inner) + 2 + 2) as u16;
    let width = (inner + 2) as u16;
    let (width, height) = (width.min(frame.width), height.min(frame.height));
    Rect {
        x: frame.x + (frame.width - width) / 2,
        y: frame.y + (frame.height - height) / 2,
        width,
        height,
    }
}

/// Draw a confirm for `prompt`, text-sized.
pub fn draw<B: Backend>(frame: &mut Frame<B>, prompt: String) {
    let area = popup_area(frame.size(), &prompt);
    frame.render_widget(Clear, area);
    frame.render_widget(confirm(prompt), area);
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
        .wrap(Wrap { trim: true })
}
