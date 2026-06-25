//! The account overlay (modal popup) showing the logged-in Steam account.

use tui::layout::Constraint;
use tui::text::Span;
use tui::widgets::{Cell, Row, Table};

use crate::browse::Browser;
use crate::theme;

/// One key-value row in the account table.
fn row(label: &str, value: String) -> Row<'static> {
    Row::new(vec![
        Cell::from(Span::styled(label.to_string(), theme::label())),
        Cell::from(Span::styled(value, theme::value())),
    ])
}

/// Build the account overlay content from the fetched account details.
pub fn account(browser: &Browser) -> Table<'static> {
    let rows = match &browser.account_info {
        Some(info) => vec![
            row("Player", info.display_name()),
            row("Login", info.account_name.clone()),
            row("SteamID", info.steam_id.to_string()),
            row("Country", info.country.clone()),
            row(
                "Email",
                format!(
                    "{} ({})",
                    info.email,
                    if info.email_validated {
                        "validated"
                    } else {
                        "unvalidated"
                    }
                ),
            ),
            row("Devices", info.authed_machines.to_string()),
            row("VAC bans", info.vac_bans.to_string()),
        ],
        None => vec![Row::new(vec![Cell::from(Span::styled(
            "No account information",
            theme::value(),
        ))])],
    };

    Table::new(rows)
        .block(theme::panel(
            "Account ([o] log out — any other key closes)".to_string(),
        ))
        .style(theme::base())
        .widths(&[Constraint::Percentage(28), Constraint::Percentage(72)])
        .column_spacing(1)
}
