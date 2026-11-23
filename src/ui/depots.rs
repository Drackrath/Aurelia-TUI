//! Depots overlay: id, name, size, config.

use pretty_bytes::converter::convert;

use tui::text::{Span, Spans};
use tui::widgets::Paragraph;

use crate::browse::Browser;
use crate::restart_depots::depot_name;
use crate::theme;
use crate::ui::paneled_paragraph;

/// Depots overlay title.
pub fn title(count: usize) -> String {
    format!("Depots ({count}) [Enter] files [Esc] close")
}

/// One depot row's spans.
pub fn depot_row(depot: &crate::interface::aurelia::DepotJson, selected: bool) -> Spans<'static> {
    let style = if selected {
        theme::selection(theme::ACCENT)
    } else {
        theme::value()
    };
    let mut spans = vec![
        Span::styled(crate::ui::selection_marker(selected), style),
        Span::styled(format!("{:>8}", depot.id), style),
    ];
    if let Some(name) = depot_name(depot.id, &depot.name) {
        spans.push(Span::styled(format!("  {name}"), style));
    }
    // Zero means Aurelia doesn't know.
    if depot.size > 0 {
        spans.push(Span::styled(
            format!("  {}", convert(depot.size as f64)),
            theme::dim(),
        ));
    }
    if !depot.config.is_empty() {
        spans.push(Span::styled(format!("  [{}]", depot.config), theme::dim()));
    }
    Spans::from(spans)
}

/// Depots list; `rows` is inner height.
pub fn depots(browser: &Browser, rows: usize) -> Paragraph<'static> {
    let lines: Vec<Spans<'static>> = if browser.depots.is_empty() {
        vec![Spans::from(Span::styled(
            "No depots.".to_string(),
            theme::dim(),
        ))]
    } else {
        let selected = browser.depots_scroll;
        let (start, end) = crate::ui::scroll::window(selected, browser.depots.len(), rows);
        browser.depots[start..end]
            .iter()
            .enumerate()
            .map(|(i, depot)| depot_row(depot, start + i == selected))
            .collect()
    };

    paneled_paragraph(lines, title(browser.depots.len()))
}
