//! The achievements overlay (modal popup): the selected game's achievements
//! with the logged-in user's unlock state, scrollable.

use tui::style::Style;
use tui::text::{Span, Spans};
use tui::widgets::Paragraph;

use crate::browse::Browser;
use crate::theme;
use crate::ui::paneled_paragraph;

/// Build the achievements overlay content from the browse state. Each row is a
/// unlock marker (✓ unlocked / ○ locked), the achievement name, and its global
/// rarity. The list is scrolled by `browser.ach_scroll`.
pub fn achievements(browser: &Browser) -> Paragraph<'static> {
    let items = &browser.achievements;
    let unlocked = items.iter().filter(|a| a.unlocked).count();
    let title = format!("Achievements ({} unlocked / {})", unlocked, items.len());

    if items.is_empty() {
        return paneled_paragraph("No achievements.", title);
    }

    let lines: Vec<Spans<'static>> = items
        .iter()
        .skip(browser.ach_scroll)
        .map(|a| {
            let (glyph, glyph_style) = if a.unlocked {
                ("✓ ", Style::default().fg(theme::ONLINE))
            } else {
                ("○ ", theme::dim())
            };
            let name = if a.visible || a.unlocked {
                a.name.clone()
            } else if a.name.is_empty() {
                "(hidden)".to_string()
            } else {
                format!("{} (hidden)", a.name)
            };
            let name_style = if a.unlocked {
                theme::value()
            } else {
                theme::dim()
            };
            Spans::from(vec![
                Span::styled(glyph, glyph_style),
                Span::styled(name, name_style),
                Span::styled(format!(" ({:.1}%)", a.rarity), theme::dim()),
            ])
        })
        .collect();

    paneled_paragraph(lines, title)
}
