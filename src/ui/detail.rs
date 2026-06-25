//! The detail pane for the selected game.

use pretty_bytes::converter::convert;

use tui::layout::Constraint;
use tui::style::Style;
use tui::text::{Span, Spans, Text};
use tui::widgets::{Block, BorderType, Borders, Cell, Row, Table};

use crate::interface::game::Game;
use crate::theme;
use crate::util::stateful::Named;

/// How many wrapped lines of the description to show while collapsed. Pressing
/// `[i]` expands the row to the full text.
const DESC_COLLAPSED_LINES: usize = 6;

/// Word-wrap `text` to `width` columns, preserving explicit newlines. Words
/// longer than `width` simply overflow their line.
fn wrap(text: &str, width: usize) -> Vec<String> {
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
fn transparent_panel() -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::BORDER).bg(theme::BG))
        .title(Spans::from(Span::styled("Detail", theme::title_style())))
}

/// An empty, height-1 row used both as a section separator and as top padding.
/// It paints nothing, so the cover art behind it stays visible.
fn blank() -> Row<'static> {
    Row::new(Vec::<Cell>::new())
}

/// Build the detail content rows for `game` and their total height. Does *not*
/// trigger the lazy metadata fetches — the caller does that only when the
/// selection has settled (so scrolling doesn't fire a fetch per game).
fn build_content(game: Option<&Game>, expand: bool, width: u16) -> (Vec<Row<'static>>, u16) {
    let mut content: Vec<Row> = Vec::new();
    let mut content_h: u16 = 0;
    let mut push = |row: Row<'static>, h: u16| {
        content.push(row);
        content_h += h;
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
            push(blank(), 1);

            // Detail rows. Developer/publisher/description are filled in lazily
            // from `aurelia info` (see Game::query_info).
            let homepage = g.homepage.clone();
            let developer = g.get_developer();
            let publisher = g.get_publisher();
            let proton = g.get_proton();

            let detail_rows: Vec<(&str, &String)> = vec![
                ("Homepage", &homepage),
                ("Developer", &developer),
                ("Publisher", &publisher),
                ("Proton Tier", &proton),
            ];
            for &(heading, value) in &detail_rows {
                push(
                    Row::new(vec![
                        Cell::from(Span::styled(heading, theme::label())),
                        Cell::from(Span::styled(value.clone(), theme::value())),
                    ]),
                    1,
                );
            }

            if let Some(status) = g.get_status() {
                push(blank(), 1);
                let size = convert(status.size);
                for &(heading, value) in &[
                    ("State", &status.state),
                    ("Installation", &status.installdir),
                    ("Size", &size),
                ] {
                    push(
                        Row::new(vec![
                            Cell::from(Span::styled(heading, theme::label())),
                            Cell::from(Span::styled(value.clone(), theme::value())),
                        ]),
                        1,
                    );
                }
            }

            // Description, listed as its own (wrapping) row.
            let description = g.get_description();
            if !description.is_empty() {
                push(blank(), 1);
                let mut lines = wrap(&description, width.max(1) as usize);
                if !expand && lines.len() > DESC_COLLAPSED_LINES {
                    lines.truncate(DESC_COLLAPSED_LINES);
                    if let Some(last) = lines.last_mut() {
                        last.push('…');
                    }
                }
                let label = if expand {
                    "Description ([i] collapse)"
                } else {
                    "Description ([i] expand)"
                };
                let lines_h = lines.len() as u16;
                let text = Text::from(
                    lines
                        .into_iter()
                        .map(|l| Spans::from(Span::styled(l, theme::value())))
                        .collect::<Vec<_>>(),
                );
                push(
                    Row::new(vec![
                        Cell::from(Span::styled(label, theme::label())),
                        Cell::from(text),
                    ])
                    .height(lines_h),
                    lines_h,
                );
            }
        }
        None => push(
            Row::new(vec![Cell::from(Span::styled(
                "No game selected",
                theme::value(),
            ))]),
            1,
        ),
    }

    (content, content_h)
}

/// The total height (rows) the detail content occupies — i.e. the height of the
/// region from the `ID | Name` head down to the bottom of the panel. The caller
/// uses this to size the cover art into the empty space above it.
pub fn content_height(game: Option<&Game>, expand: bool, width: u16) -> u16 {
    build_content(game, expand, width).1
}

/// Build the detail table for the selected game (or an empty-state table). The
/// description is listed as its own row; `expand` controls whether it is capped
/// at [`DESC_COLLAPSED_LINES`] or shown in full, and `width` is the value
/// column width used to wrap it. `height` is the panel's inner height: the rows
/// are bottom-aligned within it (padded from the top) so the cover art fills the
/// empty space above the `ID | Name` head.
pub fn detail(game: Option<&Game>, expand: bool, width: u16, height: u16) -> Table<'static> {
    let (content, content_h) = build_content(game, expand, width);

    // Bottom-align: pad the top with empty rows so the content sits against the
    // panel's lower edge, leaving the top free for the cover art.
    let pad = height.saturating_sub(content_h);
    let mut rows: Vec<Row> = Vec::with_capacity(pad as usize + content.len());
    rows.resize_with(pad as usize, blank);
    rows.extend(content);

    // No `.style(...)`: the table must not paint a background, so the cover art
    // rendered behind it remains visible between the rows.
    Table::new(rows)
        .block(transparent_panel())
        .widths(&[Constraint::Percentage(18), Constraint::Percentage(82)])
}
