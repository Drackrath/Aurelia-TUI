//! The detail pane for the selected game.

use tui::layout::Constraint;
use tui::style::Style;
use tui::text::{Span, Spans};
use tui::widgets::{Block, BorderType, Borders, Cell, Row, Table};

use crate::interface::game::Game;
use crate::theme;
use crate::util::stateful::Named;

/// How many wrapped lines of the description to show while collapsed. Pressing
/// `[i]` removes this cap; the available panel height still bounds the row.
const DESC_COLLAPSED_LINES: usize = 6;

/// Word-wrap `text` to `width` columns, preserving explicit newlines. Words
/// longer than `width` simply overflow their line.
pub(super) fn wrap(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut line = String::new();
        let mut col = 0usize;
        for word in paragraph.split_whitespace() {
            let wlen = word.chars().count();
            if col == 0 {
                line.push_str(word);
                col = wlen;
            } else if col + 1 + wlen <= width {
                line.push(' ');
                line.push_str(word);
                col += 1 + wlen;
            } else {
                lines.push(std::mem::take(&mut line));
                line.push_str(word);
                col = wlen;
            }
        }
        lines.push(line);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// A transparent Detail panel: the same rounded, accent-titled frame as
/// [`theme::panel`] but with no inner fill, so the cover art rendered behind it
/// shows through as the background.
fn transparent_panel(title: &'static str) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::BORDER).bg(theme::BG))
        .title(Spans::from(Span::styled(title, theme::title_style())))
}

/// An empty, height-1 row used both as a section separator and as top padding.
/// It paints nothing, so the cover art behind it stays visible.
fn blank() -> Row<'static> {
    Row::new(Vec::<Cell>::new())
}

/// Build the detail content rows for `game` and their total height. Does *not*
/// trigger the lazy metadata fetches — the caller does that only when the
/// selection has settled (so scrolling doesn't fire a fetch per game).
/// `None` measures the natural height for cover-art placement; `Some(height)`
/// builds for the actual panel, limiting the description to its remaining rows.
/// This is private bookkeeping: `detail` already receives the panel height.
fn build_content(
    game: Option<&Game>,
    expand: bool,
    width: u16,
    height: Option<u16>,
    scroll: u16,
) -> (Vec<Row<'static>>, u16) {
    let mut content: Vec<Row> = Vec::new();
    let mut content_h = 0u16;
    let mut push = |row: Row<'static>, h: u16| {
        content.push(row);
        content_h += h;
        content_h
    };

    match game {
        Some(g) => {
            // Table head (id, name).
            push(
                Row::new(vec![
                    Cell::from(Span::styled("ID", theme::label())),
                    Cell::from(Span::styled("Name", theme::label())),
                ]),
                1,
            );
            push(
                Row::new(vec![
                    Cell::from(Span::styled(g.id.to_string(), theme::value())),
                    Cell::from(Span::styled(g.get_name(), theme::value())),
                ]),
                1,
            );
            let start = push(blank(), 1);

            // Detail rows, filled lazily (Game::query_info).
            let narrow = super::library_info::is_narrow(width);
            let lbl = |long: &str, short: &str| super::library_info::label(long, short, narrow);
            let mut core: Vec<super::library_info::Line> = vec![
                (lbl("Homepage", "Homepage"), g.homepage.clone()),
                (lbl("Developer", "Dev"), g.get_developer()),
                (lbl("Publisher", "Pub"), g.get_publisher()),
                (lbl("Proton Tier", "Proton"), g.get_proton()),
            ];
            core.extend(super::library_info::library_lines(g));
            if let Some(cloud) = super::cloud_sync::detail_value(g.id) {
                core.push(("Cloud".into(), cloud));
            }
            // Check the preference at render time too: an in-flight lookup may
            // finish after disabling, but its result must stay hidden.
            if crate::config::Config::cached().hltb_enabled
                && let Some(hltb_lines) = super::hltb::lines(&g.get_hltb(), width as usize)
            {
                for (i, line) in hltb_lines.into_iter().enumerate() {
                    let label = if i == 0 { "HLTB" } else { "" };
                    core.push((label.into(), line));
                }
            }
            if let Some(status) = g.get_status() {
                core.push((String::new(), String::new()));
                let size = super::library_info::size_text(g, status.size);
                core.push(("State".into(), status.state.clone()));
                if !status.installdir.is_empty() {
                    core.push((lbl("Installation", "Path"), status.installdir.clone()));
                }
                if g.installed || status.size > 0.0 {
                    core.push(("Size".into(), size));
                }
            }

            let extra =
                super::library_info::extra_lines(g, width, expand, DESC_COLLAPSED_LINES, narrow);
            let budget = |used: u16| {
                height
                    .map(|h| h.saturating_sub(used) as usize)
                    .unwrap_or(usize::MAX)
            };
            if expand {
                // Everything below the head scrolls.
                let mut all = core;
                if !extra.is_empty() {
                    all.push((String::new(), String::new()));
                    all.extend(extra);
                }
                let room = budget(start).min(all.len());
                let shown =
                    super::library_info::visible_lines(all, room, scroll, true, height.is_some());
                for row in super::library_info::line_rows(shown) {
                    push(row, 1);
                }
            } else {
                for row in super::library_info::line_rows(core) {
                    push(row, 1);
                }
                if !extra.is_empty() {
                    let used = push(blank(), 1);
                    let room = budget(used).min(extra.len());
                    let shown = super::library_info::visible_lines(extra, room, 0, false, false);
                    for row in super::library_info::line_rows(shown) {
                        push(row, 1);
                    }
                }
            }
        }
        None => {
            push(
                Row::new(vec![Cell::from(Span::styled(
                    "No game selected",
                    theme::value(),
                ))]),
                1,
            );
        }
    }

    (content, content_h)
}

/// The total height (rows) the detail content occupies — i.e. the height of the
/// region from the `ID | Name` head down to the bottom of the panel. The caller
/// uses this to size the cover art into the empty space above it.
pub fn content_height(game: Option<&Game>, expand: bool, width: u16) -> u16 {
    build_content(game, expand, width, None, 0).1
}

/// Build the detail table for the selected game (or an empty-state table). The
/// description is listed as its own row; `expand` controls whether it is capped
/// at [`DESC_COLLAPSED_LINES`] or may use the remaining height; `width` is the value
/// column width used to wrap it. `height` is the panel's inner height: the rows
/// are bottom-aligned within it (padded from the top) so the cover art fills the
/// empty space above the `ID | Name` head.
pub fn detail(game: Option<&Game>, expand: bool, width: u16, height: u16) -> Table<'static> {
    detail_scrolled(game, expand, width, height, 0)
}

/// [`detail`] with expanded lines scrolled.
pub fn detail_scrolled(
    game: Option<&Game>,
    expand: bool,
    width: u16,
    height: u16,
    scroll: u16,
) -> Table<'static> {
    let (content, content_h) = build_content(game, expand, width, Some(height), scroll);

    // Bottom-align: pad the top with empty rows so the content sits against the
    // panel's lower edge, leaving the top free for the cover art.
    let pad = height.saturating_sub(content_h);
    let mut rows: Vec<Row> = Vec::with_capacity(pad as usize + content.len());
    rows.resize_with(pad as usize, blank);
    rows.extend(content);

    // No `.style(...)`: the table must not paint a background, so the cover art
    // rendered behind it remains visible between the rows.
    Table::new(rows)
        .block(transparent_panel(super::library_info::detail_title(
            expand,
            game.is_some(),
        )))
        .widths(&[Constraint::Percentage(18), Constraint::Percentage(82)])
}
