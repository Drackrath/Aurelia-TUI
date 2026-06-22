//! The Proton/Wine runtime overlay (modal popup): the available runtimes with
//! their installed/default state, and the highlighted row for set-default.

use tui::style::Modifier;
use tui::text::{Span, Spans};
use tui::widgets::{List, ListItem};

use crate::browse::Browser;
use crate::theme;

/// Build the Proton overlay list from the browser's open runtime state. Each row
/// shows the runtime name, an "installed"/"default" marker, and is dimmed when
/// not installed; the highlighted row gets a bright selection bar with a `▶`
/// marker. Renders a single "No runtimes." row when empty.
pub fn proton(browser: &Browser) -> List<'static> {
    let items: Vec<ListItem<'static>> = if browser.protons.is_empty() {
        vec![ListItem::new(Spans::from(Span::styled(
            "No runtimes.".to_string(),
            theme::dim(),
        )))]
    } else {
        browser
            .protons
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let selected = i == browser.proton_index;

                let marker = if selected { "▶ " } else { "  " };
                let name_style = if selected {
                    theme::selection(theme::ACCENT)
                } else if entry.installed {
                    theme::value()
                } else {
                    theme::dim()
                };

                let mut spans = vec![
                    Span::styled(marker.to_string(), name_style),
                    Span::styled(entry.name.clone(), name_style),
                ];

                // State markers: default (accent) and/or installed (online green).
                if entry.is_default {
                    spans.push(Span::raw("  ".to_string()));
                    spans.push(Span::styled(
                        "[default]".to_string(),
                        theme::value().fg(theme::ACCENT).add_modifier(Modifier::BOLD),
                    ));
                } else if entry.installed {
                    spans.push(Span::raw("  ".to_string()));
                    spans.push(Span::styled(
                        "[installed]".to_string(),
                        theme::value().fg(theme::ONLINE),
                    ));
                }

                ListItem::new(Spans::from(spans))
            })
            .collect()
    };

    List::new(items)
        .block(theme::panel("Proton runtimes ([d] set default)".to_string()))
        .style(theme::base())
}
