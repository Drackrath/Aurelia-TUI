//! The running-games overlay (modal popup): the games Aurelia currently has
//! running, with the highlighted row available to stop.

use tui::text::{Span, Spans};
use tui::widgets::{List, ListItem};

use crate::browse::Browser;
use crate::theme;

/// Build the running overlay list from the browser's open running state. Each
/// row shows the game name and its pid; the highlighted row gets a bright
/// selection bar with a `▶` marker. Renders a single "No games running." row
/// when empty.
pub fn running(browser: &Browser) -> List<'static> {
    let items: Vec<ListItem<'static>> = if browser.running.is_empty() {
        crate::ui::empty_list_rows("No games running.")
    } else {
        browser
            .running
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let selected = i == browser.running_index;

                let marker = crate::ui::selection_marker(selected);
                let name_style = if selected {
                    theme::selection(theme::ACCENT)
                } else {
                    theme::value().fg(theme::ONLINE)
                };

                let spans = vec![
                    Span::styled(marker.to_string(), name_style),
                    Span::styled(entry.name.clone(), name_style),
                    Span::raw("  ".to_string()),
                    Span::styled(format!("pid {}", entry.pid), theme::dim()),
                ];

                ListItem::new(Spans::from(spans))
            })
            .collect()
    };

    List::new(items)
        .block(theme::panel("Running ([s] stop)".to_string()))
        .style(theme::base())
}
