//! The install-location picker: a modal listing the Steam library folders a
//! game can be installed into (one per drive/location), each with its free
//! space, plus the game's estimated on-disk size. Drives without room are
//! marked and can't be chosen.

use pretty_bytes::converter::convert;

use tui::Frame;
use tui::backend::Backend;
use tui::style::Style;
use tui::text::{Span, Spans, Text};
use tui::widgets::Paragraph;

use crate::browse::Browser;
use crate::theme;
use crate::ui::install_options as opts_ui;

/// Picker title, hints go below.
pub const TITLE: &str = "Install into which library?";

/// Build the install-location picker from the fetched library folders, with the
/// estimated install size, each drive's free space, and the current highlight.
pub fn install_picker(browser: &Browser) -> Paragraph<'static> {
    let mut lines = head_rows(browser);
    lines.extend(library_rows(browser, 0, browser.install_libraries.len()));
    Paragraph::new(Text::from(lines))
        .block(theme::panel(TITLE.to_string()))
        .style(theme::base())
}

/// Draw sized to the frame, hints in footer.
pub fn draw<B: Backend>(frame: &mut Frame<B>, browser: &Browser) {
    let full = frame.size();
    let width = opts_ui::overlay_width(full, 96);
    let languages = !browser.install_options.languages().is_empty();
    let footer = opts_ui::hint_lines(&opts_ui::install_hints(languages), width.saturating_sub(2));
    let head = head_rows(browser);
    let libs = browser.install_libraries.len().max(1);
    let wanted = (head.len() + libs + footer.len() + 2) as u16;
    let height = wanted.min(full.height.saturating_sub(2));
    let area = opts_ui::centered(full, width, height);
    let list_h = (height as usize).saturating_sub(2 + footer.len() + head.len());
    let len = browser.install_libraries.len();
    let (start, end) = crate::ui::scroll::window(browser.install_picker_index, len, list_h);
    let mut body = head;
    body.extend(library_rows(browser, start, end));
    opts_ui::draw_panel(frame, area, theme::panel(TITLE.to_string()), body, footer);
}

fn head_rows(browser: &Browser) -> Vec<Spans<'static>> {
    opts_ui::picker_rows(&browser.install_options, browser.install_estimate)
}

/// Library rows `start..end`.
fn library_rows(browser: &Browser, start: usize, end: usize) -> Vec<Spans<'static>> {
    let estimate = browser.install_estimate;
    let mut lines: Vec<Spans<'static>> = Vec::new();
    if browser.install_libraries.is_empty() {
        lines.push(Spans::from(Span::styled(
            "No Steam library folders found.".to_string(),
            theme::dim(),
        )));
        return lines;
    }
    for (i, lib) in browser
        .install_libraries
        .iter()
        .enumerate()
        .take(end)
        .skip(start)
    {
        let selected = i == browser.install_picker_index;
        let fits = match (estimate, lib.free_bytes) {
            (Some(est), Some(free)) => free >= est,
            _ => true,
        };

        let marker = if selected { "▶ " } else { "  " };
        let name_style = if selected {
            theme::selection(theme::ACCENT)
        } else {
            theme::value()
        };
        let mut spans = vec![
            Span::styled(marker.to_string(), name_style),
            Span::styled(lib.path.clone(), name_style),
        ];

        // Free space; warn when it won't fit.
        if let Some(free) = lib.free_bytes {
            let info_style = if fits {
                theme::dim()
            } else {
                Style::default().fg(theme::WARN)
            };
            spans.push(Span::styled(
                format!("  ({} free)", convert(free as f64)),
                info_style,
            ));
            if !fits {
                spans.push(Span::styled(
                    "  ✗ not enough space".to_string(),
                    Style::default().fg(theme::WARN),
                ));
            }
        }
        lines.push(Spans::from(spans));
    }
    lines
}
