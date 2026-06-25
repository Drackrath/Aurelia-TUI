//! The install-location picker: a modal listing the Steam library folders a
//! game can be installed into (one per drive/location), each with its free
//! space, plus the game's estimated on-disk size. Drives without room are
//! marked and can't be chosen.

use pretty_bytes::converter::convert;

use tui::style::Style;
use tui::text::{Span, Spans, Text};
use tui::widgets::Paragraph;

use crate::browse::Browser;
use crate::theme;

/// Build the install-location picker from the fetched library folders, with the
/// estimated install size, each drive's free space, and the current highlight.
pub fn install_picker(browser: &Browser) -> Paragraph<'static> {
    let estimate = browser.install_estimate;
    let mut lines: Vec<Spans<'static>> = Vec::new();

    // Header: the estimated on-disk size this install needs.
    if let Some(est) = estimate {
        lines.push(Spans::from(vec![
            Span::styled("Needs ", theme::dim()),
            Span::styled(convert(est as f64), theme::value()),
            Span::styled(" on disk".to_string(), theme::dim()),
        ]));
        lines.push(Spans::from(""));
    }

    if browser.install_libraries.is_empty() {
        lines.push(Spans::from(Span::styled(
            "No Steam library folders found.".to_string(),
            theme::dim(),
        )));
    } else {
        for (i, lib) in browser.install_libraries.iter().enumerate() {
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

            // Free space, dim when it fits and warning-coloured (with a marker)
            // when the game won't fit on that drive.
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
    }

    Paragraph::new(Text::from(lines))
        .block(theme::panel(
            "Install into which library?  ([↑/↓] choose · [Enter] install · [Esc] cancel)"
                .to_string(),
        ))
        .style(theme::base())
}
