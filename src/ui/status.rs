//! The footer status bar: counts, active filter/query/sort, session, and a
//! contextual key-hint line.

use tui::layout::{Alignment, Rect};
use tui::style::Style;
use tui::text::{Span, Spans, Text};
use tui::widgets::Paragraph;

use crate::browse::Browser;
use crate::interface::{aurelia, capabilities};
use crate::theme;

/// A dim "  ·  " separator span.
fn sep() -> Span<'static> {
    Span::styled("  ·  ", theme::dim())
}

/// The footer's key hints (line 2), as `(key, description)` pairs.
const HINTS: [(&str, &str); 9] = [
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

/// Where the version badge fits in a footer `area`: the bottom row when the
/// key hints leave room for it, else the top (status) row.
pub fn version_badge_area(area: Rect, version: Option<&str>) -> Rect {
    let hints: usize = HINTS
        .iter()
        .map(|(k, d)| k.chars().count() + d.chars().count())
        .sum();
    let badge = version_badge_text(version).0.chars().count();
    let bottom = area.height > 1 && area.width as usize >= hints + badge + 2;
    let y = if bottom {
        area.y + area.height - 1
    } else {
        area.y
    };
    Rect {
        y,
        height: area.height.min(1),
        ..area
    }
}

/// The badge text, and whether it should be drawn as a warning.
fn version_badge_text(version: Option<&str>) -> (String, bool) {
    let (min_major, min_minor, min_patch) = aurelia::MIN_VERSION;
    let missing = capabilities::missing();
    match version {
        Some(v) if aurelia::version_supported(v) && !missing.is_empty() => (
            format!("aurelia v{v} (missing: {})", missing.join(", ")),
            true,
        ),
        Some(v) if aurelia::version_supported(v) => (format!("aurelia v{v}"), false),
        Some(v) => (
            format!("aurelia v{v} (needs >= {min_major}.{min_minor}.{min_patch})"),
            true,
        ),
        None => ("aurelia: not found".to_string(), true),
    }
}

/// The `aurelia` version badge for the footer's bottom-right corner: dim when
/// supported, in the warning colour (with the minimum) when too old or missing.
pub fn version_badge(version: Option<&str>) -> Paragraph<'static> {
    let (text, warn) = version_badge_text(version);
    let style = if warn {
        Style::default().fg(theme::WARN)
    } else {
        theme::dim()
    };
    Paragraph::new(Spans::from(Span::styled(text, style))).alignment(Alignment::Right)
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
    let mut line2: Vec<Span<'static>> = Vec::new();
    let hints: &[(&str, &str)] = if browser.friends_focused() {
        &crate::ui::social::FRIEND_HINTS
    } else {
        &HINTS
    };
    for (cap, desc) in hints {
        line2.push(Span::styled(cap.to_string(), theme::key()));
        line2.push(Span::styled(desc.to_string(), theme::dim()));
    }

    let text = Text::from(vec![Spans::from(line1), Spans::from(line2)]);

    Paragraph::new(text).style(theme::base())
}
