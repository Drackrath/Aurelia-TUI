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

/// Footer status bar, unclipped.
pub fn status_bar(browser: &Browser, account: Option<&str>) -> Paragraph<'static> {
    status_bar_in(browser, account, usize::MAX)
}

/// Footer fitted to `area`, beside the badge.
pub fn status_bar_fit(
    browser: &Browser,
    account: Option<&str>,
    area: Rect,
    version: Option<&str>,
) -> Paragraph<'static> {
    let badge = version_badge_area(area, version);
    let mut width = usize::from(area.width);
    if badge.y == area.y {
        // Badge shares the top row.
        width = width.saturating_sub(version_badge_text(version).0.chars().count() + 2);
    }
    status_bar_sized(browser, account, width, usize::from(area.width))
}

/// One status part; lower rank drops first.
struct Part {
    rank: u8,
    text: String,
    style: Style,
}

/// Status line parts by priority.
fn parts(browser: &Browser, account: Option<&str>) -> Vec<Part> {
    let c = browser.counts();
    let mut parts = Vec::new();
    let mut add = |rank, text: String, style| parts.push(Part { rank, text, style });
    if let Some(notice) = &browser.notice {
        add(9, notice.clone(), Style::default().fg(theme::WARN));
    }
    if browser.filtering || !browser.query.is_empty() {
        let mut q = format!("/{}", browser.query);
        if browser.filtering {
            q.push('▏');
        }
        add(8, q, theme::accent());
    }
    add(
        7,
        format!("{}/{} shown", c.visible, c.total),
        Style::default().fg(theme::TEXT),
    );
    add(
        3,
        format!("● {} installed", c.installed),
        Style::default().fg(theme::ONLINE),
    );
    add(
        4,
        format!("▲ {} updates", c.updates),
        Style::default().fg(theme::WARN),
    );
    if c.downloading > 0 {
        add(
            6,
            format!("⬇ {} downloading", c.downloading),
            Style::default().fg(theme::ACCENT_BRIGHT),
        );
    }
    add(2, format!("sort: {}", browser.sort.label()), theme::dim());
    if let Some(a) = account {
        add(1, format!("steam: {}", a), theme::accent());
    }
    parts
}

/// Width of parts joined by separators.
fn joined_width(parts: &[Part]) -> usize {
    let text: usize = parts.iter().map(|p| p.text.chars().count()).sum();
    text + parts.len().saturating_sub(1) * 5
}

/// Hints fitting `width`; help and quit stay.
pub fn fit_hints<'a>(hints: &[(&'a str, &'a str)], width: usize) -> Vec<(&'a str, &'a str)> {
    let keep = |cap: &str| cap == "[?]" || cap == "[q]";
    let len = |h: &[(&str, &str)]| -> usize {
        h.iter()
            .map(|(k, d)| k.chars().count() + d.chars().count())
            .sum()
    };
    let mut out: Vec<(&str, &str)> = hints.to_vec();
    // Drop by priority, then from right.
    const DROP: [&str; 7] = ["[r]", "[f]", "[d]", "[Tab]", "[s]", "[/]", "[Enter]"];
    while len(&out) > width {
        let ranked = DROP
            .iter()
            .find_map(|cap| out.iter().position(|(k, _)| k == cap));
        match ranked.or_else(|| out.iter().rposition(|(k, _)| !keep(k))) {
            Some(i) => {
                out.remove(i);
            }
            None => break,
        }
    }
    out
}

/// Status bar whose top line fits `width`.
pub fn status_bar_in(browser: &Browser, account: Option<&str>, width: usize) -> Paragraph<'static> {
    status_bar_sized(browser, account, width, usize::MAX)
}

/// Status bar with separate line widths.
pub fn status_bar_sized(
    browser: &Browser,
    account: Option<&str>,
    width: usize,
    hint_width: usize,
) -> Paragraph<'static> {
    let mut parts = parts(browser, account);
    // Drop low-priority parts until it fits.
    while joined_width(&parts) > width && parts.len() > 1 {
        let lowest = (0..parts.len()).min_by_key(|&i| parts[i].rank).unwrap_or(0);
        parts.remove(lowest);
    }
    if let Some(only) = parts.first_mut()
        && joined_width(std::slice::from_ref(only)) > width
    {
        let keep = width.saturating_sub(1);
        only.text = only.text.chars().take(keep).collect::<String>() + "…";
    }

    let mut line1: Vec<Span<'static>> = Vec::new();
    for (i, part) in parts.into_iter().enumerate() {
        if i > 0 {
            line1.push(sep());
        }
        line1.push(Span::styled(part.text, part.style));
    }

    // --- Line 2: key hints ---
    let mut line2: Vec<Span<'static>> = Vec::new();
    let hints: &[(&str, &str)] = if browser.friends_focused() {
        &crate::ui::social::FRIEND_HINTS
    } else {
        &HINTS
    };
    for (cap, desc) in fit_hints(hints, hint_width) {
        line2.push(Span::styled(cap.to_string(), theme::key()));
        line2.push(Span::styled(desc.to_string(), theme::dim()));
    }

    let text = Text::from(vec![Spans::from(line1), Spans::from(line2)]);

    Paragraph::new(text).style(theme::base())
}
