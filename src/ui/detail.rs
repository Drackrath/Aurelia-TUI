//! The detail pane for the selected game.

use pretty_bytes::converter::convert;

use tui::layout::Constraint;
use tui::text::Span;
use tui::widgets::{Cell, Row, Table};

use crate::interface::game::Game;
use crate::theme;
use crate::util::stateful::Named;

/// Build the detail table for the selected game (or an empty-state table).
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
            let description = g.get_description();

            let mut detail_rows: Vec<(&str, &String)> = vec![
                ("Homepage", &homepage),
                ("Developer", &developer),
                ("Publisher", &publisher),
                ("Proton Tier", &proton),
            ];
            if !description.is_empty() {
                detail_rows.push(("Description", &description));
            }
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
