//! The move (relocate install) overlay (modal popup): a text prompt where the
//! user types the destination Steam library folder. (`move` is a Rust keyword,
//! hence the module name `move_game`.)

use tui::widgets::Paragraph;

use crate::browse::Browser;

/// Build the move prompt content from the current browser state: a label, the
/// typed destination path (with a trailing caret), and the status line if any.
pub fn move_overlay(browser: &Browser) -> Paragraph<'static> {
    crate::ui::prompt_overlay(
        "Destination library folder:",
        &browser.move_path,
        &browser.move_status,
        "Move install ([Enter] confirm, [Esc] cancel)",
    )
}
