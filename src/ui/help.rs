//! The help overlay (modal popup) listing every key binding.

use tui::layout::Alignment;
use tui::style::{Modifier, Style};
use tui::text::{Span, Spans, Text};
use tui::widgets::Paragraph;

use crate::theme;

/// One entry in the help list: either a group header or a key binding.
enum Entry {
    Header(&'static str),
    Binding(&'static str, &'static str),
    Blank,
}

/// Width of a single column (the half the overlay devotes to one entry).
const COL_WIDTH: usize = 44;
/// Width of the key field within a binding column.
const KEY_WIDTH: usize = 16;

/// Render a single entry into the spans for one half of a row.
fn entry_spans(entry: &Entry) -> Vec<Span<'static>> {
    match entry {
        Entry::Header(title) => vec![Span::styled(
            format!("{:<width$}", title, width = COL_WIDTH),
            Style::default()
                .fg(theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        )],
        Entry::Binding(keys, desc) => {
            let keys_field = format!("  {:<14}", keys);
            // Clamp the description to whatever space remains in the column so
            // the two columns stay aligned regardless of entry length.
            let desc_width = COL_WIDTH.saturating_sub(KEY_WIDTH);
            let desc: String = desc.chars().take(desc_width).collect();
            vec![
                Span::styled(keys_field, theme::key()),
                Span::styled(
                    format!("{:<width$}", desc, width = desc_width),
                    Style::default().fg(theme::TEXT),
                ),
            ]
        }
        Entry::Blank => vec![Span::raw(" ".repeat(COL_WIDTH))],
    }
}

/// The full list of help entries, shared between rendering and the row count.
fn entries() -> Vec<Entry> {
    vec![
        Entry::Header("Navigation"),
        Entry::Binding("j / \u{2193}", "move down"),
        Entry::Binding("k / \u{2191}", "move up"),
        Entry::Binding("g", "jump to top"),
        Entry::Binding("G", "jump to bottom"),
        Entry::Binding("PageUp/Down", "page up / down"),
        Entry::Binding("mouse wheel", "scroll the list"),
        Entry::Binding("Shift+drag", "select text"),
        Entry::Blank,
        Entry::Header("Filtering"),
        Entry::Binding("Tab / S-Tab", "cycle tabs (+Friends)"),
        Entry::Binding("1 \u{2013} 5", "jump to a tab (5=Friends)"),
        Entry::Binding("/", "focus text filter"),
        Entry::Binding("Esc", "clear filter"),
        Entry::Binding("s", "cycle sort"),
        Entry::Blank,
        Entry::Header("Actions"),
        Entry::Binding("Enter", "launch game"),
        Entry::Binding("a", "achievements"),
        Entry::Binding("I", "inventory"),
        Entry::Binding("m", "market listings"),
        Entry::Binding("S", "search community market"),
        Entry::Binding("  Enter / Tab", "  run / price highlighted"),
        Entry::Binding("i", "expand / collapse desc"),
        Entry::Binding("d", "install (pick library)"),
        Entry::Binding("Space", "pause / resume install"),
        Entry::Binding("c", "cancel install"),
        Entry::Binding("x", "uninstall game"),
        Entry::Binding("M", "move install"),
        Entry::Binding("K", "relink install"),
        Entry::Binding("N", "import install"),
        Entry::Binding("v", "verify files"),
        Entry::Binding("U", "update game"),
        Entry::Binding("D", "manage DLC"),
        Entry::Binding("b", "beta branches"),
        Entry::Binding("o", "depots"),
        Entry::Binding("P", "proton (d/i/u)"),
        Entry::Binding("R", "running games"),
        Entry::Binding("W", "workshop items"),
        Entry::Binding("W then b", "browse workshop"),
        Entry::Binding("f", "toggle favourite"),
        Entry::Binding("H", "hide game"),
        Entry::Binding("C", "cloud saves (s/d/u)"),
        Entry::Binding("L", "launch options"),
        Entry::Binding("r", "refresh library"),
        Entry::Binding("A", "account"),
        Entry::Binding("p", "settings"),
        Entry::Binding("F / 5", "open Friends tab"),
        Entry::Binding("c / Enter", "chat (Friends tab)"),
        Entry::Binding("t", "chat in new window"),
        Entry::Binding("a", "add friend (Friends)"),
        Entry::Binding("x", "remove friend (Friends)"),
        Entry::Binding("A then o", "log out"),
        Entry::Binding("w", "wallet balance"),
        Entry::Binding("l", "sign in again"),
        Entry::Blank,
        Entry::Header("General"),
        Entry::Binding("?", "toggle this help"),
        Entry::Binding("q", "quit"),
    ]
}

/// The number of rendered rows (each row pairs two entries side by side).
pub fn row_count() -> u16 {
    entries().len().div_ceil(2) as u16
}

/// Build the help overlay content as a two-column paragraph.
///
/// `scroll` is the vertical row offset applied so the overlay can be scrolled
/// when the bindings overflow the popup.
pub fn help(scroll: u16) -> Paragraph<'static> {
    let entries = entries();

    // Split the entries into two roughly equal halves shown side by side.
    let half = entries.len().div_ceil(2);
    let (left, right) = entries.split_at(half);

    let mut lines: Vec<Spans<'static>> = Vec::with_capacity(half);
    for row in 0..half {
        let mut spans = entry_spans(&left[row]);
        spans.push(Span::raw("  "));
        if let Some(entry) = right.get(row) {
            spans.extend(entry_spans(entry));
        }
        lines.push(Spans::from(spans));
    }

    let text = Text::from(lines);

    Paragraph::new(text)
        .block(theme::panel(
            "Help \u{2014} j/k scroll \u{00b7} Esc/q/? close".to_string(),
        ))
        .style(theme::base())
        .alignment(Alignment::Left)
        .scroll((scroll, 0))
}
