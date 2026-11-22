//! The relink (relink install) overlay (modal popup): a text prompt where the
//! user types the destination Steam library folder.

use crate::browse::Browser;

/// Build the relink prompt content from the current browser state: a label, the
/// typed destination path (with a trailing caret), and the status line if any.
pub fn relink_overlay(browser: &Browser) -> crate::ui::Prompt {
    crate::ui::prompt_overlay(
        "Destination library folder:",
        &browser.relink_path,
        &browser.relink_status,
        "Relink install ([Enter] confirm, [Esc] cancel)",
    )
}
