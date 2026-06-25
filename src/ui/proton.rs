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
///
/// The panel title carries the key hints and, while a runtime install/uninstall
/// is in flight, the streamed status line (e.g. `downloading 42.0%`).
pub fn proton(browser: &Browser) -> List<'static> {
    let items: Vec<ListItem<'static>> = if browser.protons.is_empty() {
        crate::ui::empty_list_rows("No runtimes.")
    } else {
        browser
            .protons
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let selected = i == browser.proton_index;

                let marker = crate::ui::selection_marker(selected);
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

    // Title carries the key hints, plus the live install/uninstall status (if
    // any) and an [u] uninstall hint when the highlighted runtime qualifies.
    let mut title = match browser.proton_status_line() {
        Some(status) => format!("Proton runtimes — {} ([d] default · [i] install", status),
        None => "Proton runtimes ([d] default · [i] install".to_string(),
    };
    if browser.selected_proton_uninstallable() {
        title.push_str(" · [u] uninstall");
    }
    title.push(')');

    List::new(items)
        .block(theme::panel(title))
        .style(theme::base())
}
