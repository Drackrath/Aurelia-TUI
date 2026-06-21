//! Filter tab bar (All / Installed / Updates / Favourites).

use tui::text::{Span, Spans};
use tui::widgets::Tabs;

use crate::browse::{Browser, Filter};
use crate::theme;

/// Build the filter tab bar reflecting the active filter.
pub fn tabs(browser: &Browser) -> Tabs<'static> {
    let titles: Vec<Spans<'static>> = Filter::TABS
        .iter()
        .map(|f| Spans::from(Span::raw(f.label().to_string())))
        .collect();

    Tabs::new(titles)
        .select(browser.filter.index())
        .highlight_style(theme::tab_active())
        .style(theme::tab_inactive())
        .divider(Span::styled("│".to_string(), theme::dim()))
        .block(theme::panel("Library".to_string()))
}
