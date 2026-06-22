//! Browse-view widgets. Each submodule is a pure builder that renders from the
//! [`crate::browse::Browser`] state plus the shared [`crate::theme`], so the
//! event loop only has to lay them out.

pub mod cloud;
pub mod achievements;
pub mod account;
pub mod branches;
pub mod config;
pub mod confirm;
pub mod depots;
pub mod detail;
pub mod dlc;
pub mod friends;
pub mod help;
pub mod inventory;
pub mod launch;
pub mod list;
pub mod market;
pub mod move_game;
pub mod proton;
pub mod running;
pub mod status;
pub mod tabs;
pub mod wallet;
pub mod workshop;

use tui::layout::{Constraint, Direction, Layout, Rect};

/// A rectangle centered within `area`, sized as a percentage of it. Used to
/// place modal overlays (e.g. the help popup).
pub fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(vertical[1])[1]
}
