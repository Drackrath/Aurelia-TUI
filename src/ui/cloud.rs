//! The Steam Cloud overlay: lists the selected game's Cloud save files, with
//! keys to sync them — `s` both ways, `d` download-only, `u` upload-only.

use pretty_bytes::converter::convert;

use tui::layout::Constraint;
use tui::text::Span;
use tui::widgets::{Cell, Row, Table};

use crate::browse::Browser;
use crate::theme;

/// Build the Steam Cloud file table from the browser's fetched state. Each row
/// shows the file name and its (human-readable) size; a status line, when set,
/// is shown as the first row. Empty file lists render a "No cloud files." row.
pub fn cloud(browser: &Browser) -> Table<'static> {
    let mut rows: Vec<Row<'static>> = Vec::new();

    if !browser.cloud_status.is_empty() {
        rows.push(Row::new(vec![
            Cell::from(Span::styled(browser.cloud_status.clone(), theme::accent())),
            Cell::from(Span::raw("")),
        ]));
    }

    if browser.cloud_files.is_empty() {
        if browser.cloud_status.is_empty() {
            rows.push(Row::new(vec![
                Cell::from(Span::styled("No cloud files.", theme::dim())),
                Cell::from(Span::raw("")),
            ]));
        }
    } else {
        for file in &browser.cloud_files {
            rows.push(Row::new(vec![
                Cell::from(Span::styled(file.filename.clone(), theme::value())),
                Cell::from(Span::styled(convert(file.size as f64), theme::dim())),
            ]));
        }
    }

    Table::new(rows)
        .header(Row::new(vec![
            Cell::from(Span::styled("NAME", theme::label())),
            Cell::from(Span::styled("SIZE", theme::label())),
        ]))
        .block(theme::panel(
            "Steam Cloud ([s] sync  [d] down  [u] up)".to_string(),
        ))
        .style(theme::base())
        .widths(&[Constraint::Percentage(75), Constraint::Percentage(25)])
}
