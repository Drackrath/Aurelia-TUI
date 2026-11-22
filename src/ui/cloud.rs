//! The Steam Cloud saves overlay.

use pretty_bytes::converter::convert;

use tui::Frame;
use tui::backend::Backend;
use tui::layout::{Constraint, Direction, Layout, Rect};
use tui::text::{Span, Spans};
use tui::widgets::{Cell, Clear, Paragraph, Row, Table, Wrap};

use crate::browse::Browser;
use crate::interface::aurelia::CloudDirection;
use crate::theme;

/// Most status lines above the list.
const MAX_INFO: usize = 5;

/// Footer key hints.
pub const HINTS: &str = "[s] sync [d] down [u] up [j/k/g/G] move [Esc] close";

/// Status, last outcome and details.
pub fn info_lines(browser: &Browser) -> Vec<Spans<'static>> {
    let mut lines = Vec::new();
    if !browser.cloud_status.is_empty() {
        lines.push(Spans::from(Span::styled(
            browser.cloud_status.clone(),
            theme::accent(),
        )));
    }
    // Last outcome, failed and skipped.
    if let Some(record) = browser
        .selected()
        .and_then(|g| crate::interface::cloud_status::get(g.id))
    {
        let now = crate::interface::cloud_status::now();
        lines.push(Spans::from(vec![
            Span::styled(
                crate::ui::cloud_sync::describe(&record, now),
                theme::label(),
            ),
            Span::styled(
                format!("  {} failed · {} skipped", record.failed, record.skipped),
                theme::dim(),
            ),
        ]));
        for line in record.message.iter().flat_map(|m| m.lines()) {
            lines.push(Spans::from(Span::styled(line.to_string(), theme::dim())));
        }
    }
    lines.truncate(MAX_INFO);
    lines
}

/// Upload/download confirm wording.
pub fn confirm_text(direction: CloudDirection) -> (String, String) {
    match direction {
        CloudDirection::Up => (
            "Upload local saves to Steam Cloud?".to_string(),
            "Cloud copies of changed files are replaced by this PC's saves.".to_string(),
        ),
        _ => (
            "Download Steam Cloud saves to this PC?".to_string(),
            "Local copies of changed files are replaced by the Cloud's saves.".to_string(),
        ),
    }
}

/// The windowed file table.
pub fn files(browser: &Browser, height: usize) -> Table<'static> {
    let files = &browser.cloud_files;
    let selected = browser.cloud_view.index.min(files.len().saturating_sub(1));
    let (start, end) = crate::ui::scroll::window(selected, files.len(), height);
    let mut rows: Vec<Row<'static>> = Vec::new();
    if files.is_empty() && browser.cloud_status.is_empty() {
        rows.push(Row::new(vec![Cell::from(Span::styled(
            "No cloud files.",
            theme::dim(),
        ))]));
    }
    for (i, file) in files.iter().enumerate().take(end).skip(start) {
        let (name, size) = if i == selected {
            let s = theme::selection(theme::ACCENT);
            (s, s)
        } else {
            (theme::value(), theme::dim())
        };
        rows.push(Row::new(vec![
            Cell::from(Span::styled(file.filename.clone(), name)),
            Cell::from(Span::styled(convert(file.size as f64), size)),
        ]));
    }
    Table::new(rows)
        .header(Row::new(vec![
            Cell::from(Span::styled("NAME", theme::label())),
            Cell::from(Span::styled("SIZE", theme::label())),
        ]))
        .style(theme::base())
        .widths(&[Constraint::Percentage(78), Constraint::Percentage(22)])
}

/// Panel title with position.
pub fn title(browser: &Browser) -> String {
    let len = browser.cloud_files.len();
    if len == 0 {
        "Steam Cloud".to_string()
    } else {
        let at = browser.cloud_view.index.min(len - 1) + 1;
        format!("Steam Cloud · {at}/{len}")
    }
}

/// Draw the overlay into `area`.
pub fn draw<B: Backend>(frame: &mut Frame<B>, browser: &Browser, area: Rect) {
    frame.render_widget(Clear, area);
    let block = theme::panel(title(browser));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let info = info_lines(browser);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(info.len() as u16),
            Constraint::Min(2),
            Constraint::Length(1),
        ])
        .split(inner);
    frame.render_widget(
        Paragraph::new(info)
            .style(theme::base())
            .wrap(Wrap { trim: false }),
        chunks[0],
    );
    // Minus the table header row.
    let height = chunks[1].height.saturating_sub(1) as usize;
    frame.render_widget(files(browser, height), chunks[1]);
    frame.render_widget(
        Paragraph::new(Span::styled(HINTS, theme::dim())).style(theme::base()),
        chunks[2],
    );

    if let Some(direction) = browser.cloud_view.confirm {
        let (question, detail) = confirm_text(direction);
        let popup = crate::ui::centered_rect(80, 60, area);
        frame.render_widget(Clear, popup);
        frame.render_widget(
            crate::ui::restart_depots::question_box(question, detail),
            popup,
        );
    }
}
