//! The footer status bar: counts, active filter/query/sort, session, and a
//! contextual key-hint line.

use tui::style::Style;
use tui::text::{Span, Spans, Text};
use tui::widgets::Paragraph;

use crate::browse::Browser;
use crate::theme;

/// A dim "  ·  " separator span.
fn sep() -> Span<'static> {
    Span::styled("  ·  ", theme::dim())
}

/// Build the footer status bar (a borderless two-line footer).
pub fn status_bar(browser: &Browser, account: Option<&str>) -> Paragraph<'static> {
    let c = browser.counts();

    // --- Line 1: status ---
    let mut line1: Vec<Span<'static>> = Vec::new();

    // A transient notice (e.g. a failed action) takes the front of the line in
    // the warning colour until the next keypress clears it.
    if let Some(notice) = &browser.notice {
        line1.push(Span::styled(
            notice.clone(),
            Style::default().fg(theme::WARN),
        ));
        line1.push(sep());
    }

    // Live query, shown prominently when filtering or a query is present.
    if browser.filtering || !browser.query.is_empty() {
        let mut q = format!("/{}", browser.query);
        if browser.filtering {
            q.push('▏');
        }
        line1.push(Span::styled(q, theme::accent()));
        line1.push(sep());
    }

    line1.push(Span::styled(
        format!("{}/{} shown", c.visible, c.total),
        Style::default().fg(theme::TEXT),
    ));

    line1.push(sep());
    line1.push(Span::styled(
        format!("● {} installed", c.installed),
        Style::default().fg(theme::ONLINE),
    ));

    line1.push(sep());
    line1.push(Span::styled(
        format!("▲ {} updates", c.updates),
        Style::default().fg(theme::WARN),
    ));

    if c.downloading > 0 {
        line1.push(sep());
        line1.push(Span::styled(
            format!("⬇ {} downloading", c.downloading),
            Style::default().fg(theme::ACCENT_BRIGHT),
        ));
    }

    line1.push(sep());
    line1.push(Span::styled(
        format!("sort: {}", browser.sort.label()),
        theme::dim(),
    ));

    if let Some(a) = account {
        line1.push(sep());
        line1.push(Span::styled(format!("steam: {}", a), theme::accent()));
    }

    // --- Line 2: key hints ---
    let hints: [(&str, &str); 9] = [
        ("[/]", " filter  "),
        ("[Tab]", " view  "),
        ("[s]", " sort  "),
        ("[Enter]", " actions  "),
        ("[d]", " install  "),
        ("[f]", " fav  "),
        ("[r]", " refresh  "),
        ("[?]", " help  "),
        ("[q]", " quit"),
    ];
    let mut line2: Vec<Span<'static>> = Vec::new();
    for (cap, desc) in hints {
        line2.push(Span::styled(cap.to_string(), theme::key()));
        line2.push(Span::styled(desc.to_string(), theme::dim()));
    }

    let text = Text::from(vec![Spans::from(line1), Spans::from(line2)]);

    Paragraph::new(text).style(theme::base())
}
