//! The import (register existing install) overlay (modal popup): a text prompt
//! where the user types the library folder that already holds the game's files.

use tui::widgets::Paragraph;

use crate::browse::Browser;

/// Build the import prompt content from the current browser state: a label, the
/// typed library path (with a trailing caret), and the status line if any.
pub fn import_overlay(browser: &Browser) -> Paragraph<'static> {
    crate::ui::prompt_overlay(
        "Library folder with the existing files:",
        &browser.import_path,
        &browser.import_status,
        "Import install ([Enter] confirm, [Esc] cancel)",
    )
}
