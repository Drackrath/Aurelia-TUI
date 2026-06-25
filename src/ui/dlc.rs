//! The DLC management overlay (modal popup): the selected game's DLC with their
//! ownership/install state, and the highlighted row for enable/disable.

use tui::style::Modifier;
use tui::text::{Span, Spans};
use tui::widgets::{List, ListItem};

use crate::browse::Browser;
use crate::theme;

/// Build the DLC overlay list from the browser's open DLC state. Each row shows
/// the DLC name and a colour-coded status; the highlighted row gets a bright
/// selection bar with a `▶` marker. Renders a single "No DLC." row when empty.
pub fn dlc(browser: &Browser) -> List<'static> {
    let items: Vec<ListItem<'static>> = if browser.dlc.is_empty() {
        crate::ui::empty_list_rows("No DLC.")
    } else {
        browser
            .dlc
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let selected = i == browser.dlc_index;

                // State label + colour: disabled (off) wins, then installed,
                // then owned-not-installed, else not owned.
                let (state, state_style) = if entry.is_disabled() {
                    ("disabled", theme::dim().add_modifier(Modifier::BOLD))
                } else if entry.is_installed() {
                    ("installed", theme::value().fg(theme::ONLINE))
                } else if entry.is_owned() {
                    ("owned", theme::value())
                } else {
                    ("not owned", theme::dim())
                };

                let marker = crate::ui::selection_marker(selected);
                let name_style = if selected {
                    theme::selection(theme::ACCENT)
                } else {
                    theme::value()
                };

                let spans = vec![
                    Span::styled(marker.to_string(), name_style),
                    Span::styled(entry.display_name(), name_style),
                    Span::raw("  ".to_string()),
                    Span::styled(format!("[{}]", state), state_style),
                ];

                ListItem::new(Spans::from(spans))
            })
            .collect()
    };

    List::new(items)
        .block(theme::panel("DLC ([e] enable [x] disable)".to_string()))
        .style(theme::base())
}
