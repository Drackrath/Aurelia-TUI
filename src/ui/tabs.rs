//! The top bar, split into two sections: the library filter tabs (All /
//! Installed / Updates / Favourites) on the left, and the Friends & Chat tab —
//! which swaps the main list for the Friends panel — in its own box on the
//! right. Each section carries its own headline and highlights only when it is
//! the active view.

use tui::text::{Span, Spans};
use tui::widgets::Tabs;

use crate::browse::{Browser, Filter, View};
use crate::theme;

/// The library filter tabs (left section). The active filter is highlighted
/// only while the Library view is active; in the Friends view the section is
/// shown un-highlighted so the focus reads as being on the Friends box.
pub fn library_tabs(browser: &Browser) -> Tabs<'static> {
    let active = browser.view == View::Library;

    // tui's `Tabs` only styles the *selected* tab (via `highlight_style`); every
    // other tab keeps the bright base text regardless of `.style()`. So when the
    // section is inactive we must dim each title span explicitly — otherwise only
    // the selected filter greys out (e.g. just "Favourites") while the rest stay
    // bright.
    let titles: Vec<Spans<'static>> = Filter::TABS
        .iter()
        .map(|f| {
            let label = f.label().to_string();
            let span = if active {
                Span::raw(label)
            } else {
                Span::styled(label, theme::tab_inactive())
            };
            Spans::from(span)
        })
        .collect();

    Tabs::new(titles)
        .select(browser.filter.index())
        .highlight_style(if active {
            theme::tab_active()
        } else {
            theme::tab_inactive()
        })
        .style(theme::tab_inactive())
        .divider(Span::styled("│".to_string(), theme::dim()))
        .block(theme::panel("Library".to_string()))
}

/// The Friends & Chat tab (right section), headlined "Friends, Chat". It
/// highlights only while the Friends view is active.
pub fn friends_tabs(browser: &Browser) -> Tabs<'static> {
    let titles = vec![Spans::from(Span::raw("Friends".to_string()))];

    let active = browser.view == View::Friends;
    Tabs::new(titles)
        .select(0)
        .highlight_style(if active {
            theme::tab_active()
        } else {
            theme::tab_inactive()
        })
        .style(theme::tab_inactive())
        .block(theme::panel("Friends, Chat".to_string()))
}
