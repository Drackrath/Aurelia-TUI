//! Install queue overlay and limit row.

use tui::style::{Modifier, Style};
use tui::text::{Span, Spans, Text};
use tui::widgets::Paragraph;

use crate::browse::Browser;
use crate::interface::install_queue::{self as iq, QueueEntry};
use crate::theme;

fn state_style(entry: &QueueEntry) -> Style {
    match entry.state.as_str() {
        "running" => Style::default().fg(theme::ONLINE),
        "interrupted" => Style::default().fg(theme::WARN),
        "aborted" => theme::dim(),
        _ => theme::value(),
    }
}

/// Bytes, total and percent.
pub fn progress_text(entry: &QueueEntry) -> String {
    format!(
        "{} / {} ({:.1}%)",
        iq::human_bytes(entry.downloaded_bytes),
        iq::human_bytes(entry.total_bytes),
        entry.progress_percent()
    )
}

/// Queue overlay body.
pub fn install_queue(browser: &Browser) -> Paragraph<'static> {
    let q = &browser.install_queue;
    let mut lines: Vec<Spans<'static>> = Vec::new();
    if q.entries.is_empty() {
        let text = if q.loaded {
            "No installs running or in the backlog."
        } else {
            "Loading install queue…"
        };
        lines.push(Spans::from(Span::styled(text.to_string(), theme::dim())));
    }
    for (i, entry) in q.entries.iter().enumerate() {
        let selected = i == q.index;
        let name_style = if selected {
            theme::selection(theme::ACCENT)
        } else {
            theme::value()
        };
        lines.push(Spans::from(vec![
            Span::styled(
                crate::ui::selection_marker(selected).to_string(),
                name_style,
            ),
            Span::styled(entry.label(), name_style),
            Span::raw("  "),
            Span::styled(format!("{:<11}", entry.state), state_style(entry)),
            Span::styled(progress_text(entry), theme::dim()),
        ]));
        let note = entry.note();
        if !note.is_empty() {
            lines.push(Spans::from(vec![
                Span::raw("    "),
                Span::styled(note, Style::default().fg(theme::WARN)),
            ]));
        }
    }
    if !q.status.is_empty() {
        lines.push(Spans::from(""));
        lines.push(Spans::from(Span::styled(
            q.status.clone(),
            Style::default().fg(theme::WARN).add_modifier(Modifier::DIM),
        )));
    }
    crate::ui::paneled_paragraph(
        Text::from(lines),
        "Install queue ([p] pause · [r] resume · [s] stop · [f] forget · [F] forget + delete files)"
            .to_string(),
    )
}

/// Settings value for the limit.
pub fn limit_value(browser: &Browser) -> String {
    let limit = &browser.download_limit;
    if let Some(input) = &limit.input {
        let hint = match iq::parse_limit(input) {
            Ok(0) => "off".to_string(),
            Ok(k) => iq::limit_rate(k),
            Err(_) => "kbit/s or off".to_string(),
        };
        return format!("{input}_  → {hint}");
    }
    match limit.kbit {
        Some(k) => iq::describe_limit(k),
        None => "—".to_string(),
    }
}
