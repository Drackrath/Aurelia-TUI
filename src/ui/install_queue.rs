//! Install queue overlay and limit row.

use tui::Frame;
use tui::backend::Backend;
use tui::layout::{Constraint, Direction, Layout, Rect};
use tui::style::{Modifier, Style};
use tui::text::{Span, Spans, Text};
use tui::widgets::{Clear, Paragraph, Wrap};

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

/// Footer hint lines.
pub const HINTS: [&str; 2] = [
    "[j/k] move · [p] pause · [r] resume · [s] stop",
    "[f] forget · [F] forget + delete files · [Esc] close",
];

fn entry_lines(entry: &QueueEntry, selected: bool, width: usize) -> Vec<Spans<'static>> {
    let name_style = if selected {
        theme::selection(theme::ACCENT)
    } else {
        theme::value()
    };
    let progress = progress_text(entry);
    // Name yields room to progress.
    let room = width.saturating_sub(2 + 2 + 11 + progress.chars().count());
    let mut name = entry.label();
    if name.chars().count() > room {
        name = name
            .chars()
            .take(room.saturating_sub(1))
            .collect::<String>()
            + "…";
    }
    let mut lines = vec![Spans::from(vec![
        Span::styled(
            crate::ui::selection_marker(selected).to_string(),
            name_style,
        ),
        Span::styled(name, name_style),
        Span::raw("  "),
        Span::styled(format!("{:<11}", entry.state), state_style(entry)),
        Span::styled(progress, theme::dim()),
    ])];
    let note = entry.note();
    if !note.is_empty() {
        lines.push(Spans::from(vec![
            Span::raw("    "),
            Span::styled(note, Style::default().fg(theme::WARN)),
        ]));
    }
    lines
}

/// Draw the queue overlay into `area`.
pub fn render<B: Backend>(f: &mut Frame<B>, area: Rect, browser: &Browser) {
    let q = &browser.install_queue;
    let block = theme::panel("Install queue".to_string());
    let inner = block.inner(area);
    f.render_widget(Clear, area);
    f.render_widget(block.style(theme::base()), area);

    let mut footer: Vec<Spans<'static>> = Vec::new();
    let notice = browser.queue_prompt().map(|p| (p, theme::accent()));
    let notice = notice.or_else(|| {
        (!q.status.is_empty()).then(|| {
            let style = Style::default().fg(theme::WARN).add_modifier(Modifier::DIM);
            (q.status.clone(), style)
        })
    });
    if let Some((text, style)) = notice {
        footer.push(Spans::from(Span::styled(text, style)));
    }
    for hint in HINTS {
        footer.push(Spans::from(Span::styled(hint.to_string(), theme::key())));
    }
    let width = inner.width.max(1) as usize;
    // Wrapped footer height, roughly.
    let footer_h: u16 = footer
        .iter()
        .map(|l| (l.width().max(1).div_ceil(width)) as u16)
        .sum();
    let footer_h = footer_h.min(inner.height);
    let parts = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(footer_h)])
        .split(inner);

    let mut lines: Vec<Spans<'static>> = Vec::new();
    if q.entries.is_empty() {
        let text = if q.loaded {
            "No installs running or in the backlog."
        } else {
            "Loading install queue…"
        };
        lines.push(Spans::from(Span::styled(text.to_string(), theme::dim())));
    }
    // Each entry takes up to two lines.
    let rows = (parts[0].height as usize / 2).max(1);
    let (start, end) = crate::ui::scroll::window(q.index, q.entries.len(), rows);
    for (i, entry) in q.entries.iter().enumerate().take(end).skip(start) {
        lines.extend(entry_lines(entry, i == q.index, width));
    }
    f.render_widget(Paragraph::new(Text::from(lines)), parts[0]);
    f.render_widget(
        Paragraph::new(Text::from(footer)).wrap(Wrap { trim: true }),
        parts[1],
    );
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
