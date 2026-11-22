//! The help overlay listing every key.

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
const COL_WIDTH: usize = 46;
/// Width of the key field within a binding column.
const KEY_WIDTH: usize = 18;

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
            let keys_field = format!("  {:<16}", keys);
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
        Entry::Binding("j / k / \u{2193}\u{2191}", "move"),
        Entry::Binding("g / G", "top / bottom"),
        Entry::Binding("PageUp/Down", "page"),
        Entry::Binding("mouse wheel", "scroll the list"),
        Entry::Binding("Shift+drag", "select text"),
        Entry::Blank,
        Entry::Header("Tabs & filter"),
        Entry::Binding("Tab / S-Tab", "cycle tabs (+Friends)"),
        Entry::Binding("1 \u{2013} 4", "library tabs"),
        Entry::Binding("5 / F", "Friends tab (F toggles)"),
        Entry::Binding("/", "text filter (Esc clears)"),
        Entry::Binding("s", "cycle sort"),
        Entry::Blank,
        Entry::Header("Selected game"),
        Entry::Binding("Enter", "Actions menu (everything)"),
        Entry::Binding("d", "install (picker)"),
        Entry::Binding("Space", "pause / resume install"),
        Entry::Binding("c", "cancel install"),
        Entry::Binding("U / v", "update / verify"),
        Entry::Binding("x", "uninstall"),
        Entry::Binding("M / K / N", "move / relink / import"),
        Entry::Binding("V", "versions & pinning"),
        Entry::Binding("b", "beta branches"),
        Entry::Binding("o", "depots (Enter: files)"),
        Entry::Binding("D", "DLC"),
        Entry::Binding("C", "cloud saves"),
        Entry::Binding("L", "launch options"),
        Entry::Binding("W", "workshop"),
        Entry::Binding("a", "achievements"),
        Entry::Binding("I", "inventory"),
        Entry::Binding("O", "collections"),
        Entry::Binding("f / H", "favourite / hide"),
        Entry::Binding("i", "expand description"),
        Entry::Binding(",ct", "HowLongToBeat page"),
        Entry::Blank,
        Entry::Header("App"),
        Entry::Binding("p", "settings"),
        Entry::Binding("A", "account (o, y: log out)"),
        Entry::Binding("w", "wallet balance"),
        Entry::Binding("B", "install queue"),
        Entry::Binding("P", "Proton runtimes"),
        Entry::Binding("E", "runtime plugins"),
        Entry::Binding("R", "running games"),
        Entry::Binding("m", "market listings"),
        Entry::Binding("S", "search market"),
        Entry::Binding("r", "refresh library"),
        Entry::Binding("l", "sign-in screen"),
        Entry::Binding("?", "this help"),
        Entry::Binding("q", "quit"),
        Entry::Blank,
        Entry::Header("Every overlay"),
        Entry::Binding("j / k", "move / scroll"),
        Entry::Binding("Enter", "choose / change"),
        Entry::Binding("Esc", "cancel input / close"),
        Entry::Binding("q", "close (lists only)"),
        Entry::Blank,
        Entry::Header("Settings (p)"),
        Entry::Binding("Enter / Space", "toggle, cycle or edit"),
        Entry::Binding("c", "clear proxy"),
        Entry::Binding("h t l b", "HLTB/TUI cloud/lang/bin"),
        Entry::Binding("o w g d", "presence/wait/cloud/limit"),
        Entry::Binding("s a e", "shaders/anti-cheat/proxy"),
        Entry::Blank,
        Entry::Header("Friends tab"),
        Entry::Binding("Enter / c", "chat"),
        Entry::Binding("t", "chat in new window"),
        Entry::Binding("a / x", "add / remove friend"),
        Entry::Binding("i", "invite to game"),
        Entry::Binding("p / m", "friend / own profile"),
        Entry::Binding("Esc / q / F", "back to library"),
        Entry::Blank,
        Entry::Header("Game overlays"),
        Entry::Binding("Play as: d a i", "default/auto/runtime"),
        Entry::Binding("Versions: p u", "pin / unpin"),
        Entry::Binding("Versions: d s", "downgrade / SteamDB"),
        Entry::Binding("Game set.: x", "remove launch script"),
        Entry::Binding("Install: p", "cycle platform"),
        Entry::Binding("Install: ←/→ h/l", "cycle language"),
        Entry::Binding("DLC: e / x", "enable / disable"),
        Entry::Binding("Branches: s", "switch branch"),
        Entry::Binding("Cloud: s d u", "sync / down / up"),
        Entry::Binding("Depot files: /", "filter"),
        Entry::Binding("Depot files: b d", "branch / download"),
        Entry::Binding("Workshop: b", "browse / search"),
        Entry::Binding("Browse: Tab", "(un)subscribe"),
        Entry::Binding("Browse: F1 F2", "rate up / down"),
        Entry::Binding("Browse: F3", "comments"),
        Entry::Blank,
        Entry::Header("App overlays"),
        Entry::Binding("Queue: p r s", "pause/resume/stop"),
        Entry::Binding("Queue: f / F", "forget / +delete files"),
        Entry::Binding("Proton: d i u", "default/install/remove"),
        Entry::Binding("Proton: e / b", "install EAC / BattlEye"),
        Entry::Binding("Plugins: e d x", "enable/disable/remove"),
        Entry::Binding("Plugins: i U r", "install/update/repair"),
        Entry::Binding("Running: s / x", "stop game"),
        Entry::Binding("Collect.: a r", "add / remove game"),
        Entry::Binding("Collect.: n x", "new / delete"),
        Entry::Binding("Collect.: P U S", "pull / push / sync"),
        Entry::Binding("Market srch: Tab", "price highlighted"),
        Entry::Binding("Log: g G r", "top / end / reload"),
        Entry::Binding("Invite: Tab", "switch field"),
        Entry::Binding("Add friend Enter", "search, then send"),
        Entry::Blank,
        Entry::Header("Prompts"),
        Entry::Binding("y", "confirm"),
        Entry::Binding("Uninstall: p", "toggle delete prefix"),
        Entry::Binding("cloud: c / l / n", "cloud / local / leave"),
        Entry::Binding("any other key", "cancel"),
        Entry::Blank,
        Entry::Header("Sign-in"),
        Entry::Binding("Enter", "username & password"),
        Entry::Binding("y", "QR code"),
        Entry::Binding("r", "re-check session"),
        Entry::Binding("Esc", "back"),
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
