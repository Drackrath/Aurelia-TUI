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
            row(
                "Daemon logon",
                info.daemon_logon_id
                    .map_or_else(|| "— (no daemon)".to_string(), |id| id.to_string()),
            ),
        ],
        None => match (&browser.account_load.error, browser.account_load.loading) {
            (Some(error), _) => vec![row("Failed", error.clone())],
            (None, true) => vec![row("Loading…", "persona lookup may take 30 s".into())],
            (None, false) => vec![row("Account", "No account information".into())],
        },
    };

    Table::new(rows)
        .block(theme::panel(if browser.confirm_logout {
            "Log out? [y] yes · other key cancels".to_string()
        } else {
            "Account — [r] reload · [o] log out · [Esc] close".to_string()
        }))
        .style(theme::base())
        .widths(&[Constraint::Percentage(28), Constraint::Percentage(72)])
        .column_spacing(1)
}
