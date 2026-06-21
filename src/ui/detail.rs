//! The detail pane for the selected game.

use pretty_bytes::converter::convert;

use tui::layout::Constraint;
use tui::text::Span;
use tui::widgets::{Cell, Paragraph, Row, Table, Wrap};

use crate::interface::game::Game;
use crate::theme;
use crate::util::stateful::Named;

/// Panel height that fits every key-value row the detail table can show
/// (ID/Name, spacer, Homepage/Developer/Publisher/Proton, spacer,
/// State/Installation/Size = 11 rows) plus the top/bottom border.
pub const TABLE_HEIGHT: u16 = 13;

/// The selected game's description (or empty), shown in its own wrapped panel
/// below the table. `expanded` only affects the title hint; the caller sizes the
/// area (collapsed = capped at ~10 lines, expanded = as much as fits).
pub fn description(game: Option<&Game>, expanded: bool) -> Paragraph<'static> {
    let text = game.map(|g| g.get_description()).unwrap_or_default();
    let title = if expanded {
        "Description ([i] collapse)"
    } else {
        "Description ([i] expand)"
    };
    Paragraph::new(text)
        .block(theme::panel(title.to_string()))
        .style(theme::base())
        .wrap(Wrap { trim: false })
}

/// Build the detail table for the selected game (or an empty-state table). The
/// description is rendered separately by [`description`].
pub fn detail(game: Option<&Game>) -> Table<'static> {
    match game {
        Some(g) => {
            // Kick off the lazy background fetches for the Proton tier and the
            // store metadata (developer/publisher/description).
            g.query_proton();
            g.query_info();

            let spacer = Row::new(vec![Cell::from(Span::raw(" "))]);

            // Table head (id, name).
            let mut rows = vec![
                Row::new(vec![
                    Cell::from(Span::styled("ID", theme::label())),
                    Cell::from(Span::styled("Name", theme::label())),
                ]),
                Row::new(vec![
                    Cell::from(Span::styled(g.id.to_string(), theme::value())),
                    Cell::from(Span::styled(g.get_name(), theme::value())),
                ]),
                spacer.clone(),
            ];

            // Detail rows. Developer/publisher/description are filled in lazily
            // from `aurelia info` (see Game::query_info).
            let homepage = g.homepage.clone();
            let developer = g.get_developer();
            let publisher = g.get_publisher();
            let proton = g.get_proton();

            let detail_rows: Vec<(&str, &String)> = vec![
                ("Homepage", &homepage),
                ("Developer", &developer),
                ("Publisher", &publisher),
                ("Proton Tier", &proton),
            ];
            for &(heading, value) in &detail_rows {
                rows.push(Row::new(vec![
                    Cell::from(Span::styled(heading, theme::label())),
                    Cell::from(Span::styled(value.clone(), theme::value())),
                ]));
            }

            if let Some(status) = g.get_status() {
                rows.push(spacer.clone());
                let size = convert(status.size);
                for &(heading, value) in &[
                    ("State", &status.state),
                    ("Installation", &status.installdir),
                    ("Size", &size),
                ] {
                    rows.push(Row::new(vec![
                        Cell::from(Span::styled(heading, theme::label())),
                        Cell::from(Span::styled(value.clone(), theme::value())),
                    ]));
                }
            }

            Table::new(rows)
                .block(theme::panel("Detail".to_string()))
                .style(theme::base())
                .widths(&[Constraint::Percentage(18), Constraint::Percentage(82)])
        }
        None => Table::new(vec![Row::new(vec![Cell::from(Span::styled(
            "No game selected",
            theme::value(),
        ))])])
        .block(theme::panel("Detail".to_string()))
        .style(theme::base()),
    }
}
