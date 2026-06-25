//! The Workshop overlay (modal popup). Two modes:
//!
//! * the subscribed-items list (default): the selected game's subscribed /
//!   installed Workshop items, scrollable; and
//! * a browse/search pane (toggled with `b`): a query input over a list of
//!   discoverable Workshop items, with subscribe/unsubscribe on the highlight.
//!
//! Both are pure builders from `browse::Browser` + `theme`.

use pretty_bytes::converter::convert;

use tui::style::Style;
use tui::text::{Span, Spans, Text};
use tui::widgets::Paragraph;

use crate::browse::Browser;
use crate::theme;

/// Build the Workshop overlay content. Dispatches to the browse pane or the
/// subscribed-items list depending on the overlay mode.
pub fn workshop(browser: &Browser) -> Paragraph<'static> {
    if browser.workshop_browse {
        if browser.workshop_comments_open {
            comments(browser)
        } else {
            browse(browser)
        }
    } else {
        subscribed(browser)
    }
}

/// The comments sub-pane: the item id in the title, a status/spinner line, then
/// each comment as an author header followed by its body. Scrolled by
/// `browser.workshop_comments_scroll`.
fn comments(browser: &Browser) -> Paragraph<'static> {
    let title = format!(
        "Comments — item {} (Esc: back)",
        browser.workshop_comments_id()
    );
    let mut lines: Vec<Spans<'static>> = Vec::new();

    if browser.workshop_comments_loading {
        lines.push(Spans::from(Span::styled(
            "Loading comments…",
            theme::accent(),
        )));
    } else if !browser.workshop_comments_status.is_empty() {
        lines.push(Spans::from(Span::styled(
            browser.workshop_comments_status.clone(),
            theme::dim(),
        )));
    } else {
        lines.push(Spans::from(Span::styled(
            format!("{} comment(s)", browser.workshop_comments.len()),
            theme::dim(),
        )));
    }
    lines.push(Spans::from(""));

    for comment in browser
        .workshop_comments
        .iter()
        .skip(browser.workshop_comments_scroll)
    {
        lines.push(Spans::from(Span::styled(
            comment.display_author(),
            theme::accent(),
        )));
        lines.push(Spans::from(Span::styled(
            comment.message.clone(),
            theme::value(),
        )));
        lines.push(Spans::from(""));
    }

    Paragraph::new(Text::from(lines))
        .block(theme::panel(title))
        .style(theme::base())
}

/// The subscribed/installed items list. Each row is a state marker
/// (● installed / ○ subscribed-only), the item title, and its size when known.
/// The list is scrolled by `browser.workshop_scroll`.
fn subscribed(browser: &Browser) -> Paragraph<'static> {
    let items = &browser.workshop;
    let title = format!("Workshop ({}) — b: browse", items.len());

    if items.is_empty() {
        return Paragraph::new(Text::from(vec![
            Spans::from(Span::styled("No workshop items.", theme::base())),
            Spans::from(Span::styled(
                "Press 'b' to browse and subscribe.",
                theme::dim(),
            )),
        ]))
        .block(theme::panel(title))
        .style(theme::base());
    }

    let lines: Vec<Spans<'static>> = items
        .iter()
        .skip(browser.workshop_scroll)
        .map(|item| {
            let (glyph, glyph_style) = if item.installed {
                ("● ", Style::default().fg(theme::ONLINE))
            } else {
                ("○ ", theme::dim())
            };
            let mut spans = vec![
                Span::styled(glyph, glyph_style),
                Span::styled(item.display_title(), theme::value()),
            ];
            if item.file_size > 0 {
                spans.push(Span::styled(
                    format!(" ({})", convert(item.file_size as f64)),
                    theme::dim(),
                ));
            }
            Spans::from(spans)
        })
        .collect();

    Paragraph::new(Text::from(lines))
        .block(theme::panel(title))
        .style(theme::base())
}

/// The browse/search pane: a query input line, a status/hint line, then the
/// result rows with the highlight marked and each row tagged subscribed/not.
fn browse(browser: &Browser) -> Paragraph<'static> {
    let title = "Workshop browse — Enter: search, Tab: sub, F1/F2: rate, F3: comments, Esc: back"
        .to_string();
    let mut lines: Vec<Spans<'static>> = Vec::new();

    // Query input line.
    lines.push(Spans::from(vec![
        Span::styled("Search: ", theme::dim()),
        Span::styled(browser.workshop_query.clone(), theme::value()),
        Span::styled("_", theme::accent()),
    ]));

    // Status / progress / hint line.
    let status = if browser.workshop_searching {
        Span::styled("Searching…", theme::accent())
    } else if !browser.workshop_status.is_empty() {
        Span::styled(browser.workshop_status.clone(), theme::dim())
    } else {
        Span::styled(
            format!("{} result(s)", browser.workshop_results.len()),
            theme::dim(),
        )
    };
    lines.push(Spans::from(status));
    lines.push(Spans::from(""));

    if browser.workshop_results.is_empty() {
        lines.push(Spans::from(Span::styled(
            "Type a query and press Enter to search the Workshop.",
            theme::dim(),
        )));
    } else {
        for (i, item) in browser.workshop_results.iter().enumerate() {
            let selected = i == browser.workshop_index;
            let marker = if selected { "> " } else { "  " };
            let (tag, tag_style) = if item.subscribed {
                ("[subscribed] ", Style::default().fg(theme::ONLINE))
            } else {
                ("[          ] ", theme::dim())
            };
            let title_style = if selected {
                theme::accent()
            } else {
                theme::value()
            };
            let mut spans = vec![
                Span::styled(marker, theme::accent()),
                Span::styled(tag, tag_style),
                Span::styled(item.display_title(), title_style),
            ];
            if item.file_size > 0 {
                spans.push(Span::styled(
                    format!(" ({})", convert(item.file_size as f64)),
                    theme::dim(),
                ));
            }
            lines.push(Spans::from(spans));
        }
    }

    Paragraph::new(Text::from(lines))
        .block(theme::panel(title))
        .style(theme::base())
}
