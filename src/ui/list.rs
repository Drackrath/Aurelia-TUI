//! The game list with status badges.

use tui::text::{Span, Spans};
use tui::widgets::{List, ListItem};

use crate::browse::{badge, Browser};
use crate::theme;
use crate::util::stateful::Named;

/// Width of the highlight symbol ("▶ ") the `List` reserves on every row.
const HIGHLIGHT_W: usize = 2;
/// Width of the 1-cell status glyph plus its 1-cell trailing space.
const GLYPH_W: usize = 2;
/// Columns reserved before the game name on each row (highlight symbol + glyph).
const NAME_PREFIX: usize = HIGHLIGHT_W + GLYPH_W;

/// Build the game list for the current view (badges + names).
///
/// `width` is the list pane's full width. Only the highlighted row (the one the
/// `▶` cursor is hovering) marquees an over-long name — stepped by `marquee_tick`
/// (a monotonic animation counter advanced a few times a second by the caller);
/// every other row simply clips its name so the list stays calm.
pub fn list(browser: &Browser, width: u16, marquee_tick: usize) -> List<'static> {
    let inner = (width as usize).saturating_sub(2); // minus the block borders
    let selected = browser.selected_index();

    let items: Vec<ListItem<'static>> = browser
        .visible()
        .iter()
        .enumerate()
        .map(|(i, game)| {
            let b = badge(game);
            let is_selected = Some(i) == selected;

            // Pick the name style: muted when not installed ("○"), otherwise
            // installed; updates/failures override based on the badge note.
            let name_style = if b.note.as_deref() == Some("update") {
                theme::item_update()
            } else if b.note.as_deref() == Some("failed") {
                theme::item_failed()
            } else if b.glyph == "○" {
                theme::item_muted()
            } else {
                theme::item_installed()
            };

            // Downloading (and paused) rows pin the percent to the right edge so
            // it is always visible — even when this row is not the hovered one —
            // while the name fills the space to its left (marquee'd while
            // hovered).
            if b.glyph == "⬇" || b.glyph == "⏸" {
                if let Some(pct) = b.note.clone() {
                    let content_w = inner.saturating_sub(HIGHLIGHT_W);
                    let pct_w = pct.chars().count();
                    // Leave room for "<glyph><space><name><gap><pct>".
                    let name_budget = content_w.saturating_sub(GLYPH_W + pct_w + 1);
                    let raw = game.get_name();
                    let name = if is_selected {
                        marquee(&raw, name_budget, marquee_tick)
                    } else {
                        clip(&raw, name_budget)
                    };
                    let pad =
                        content_w.saturating_sub(GLYPH_W + name.chars().count() + pct_w);
                    return ListItem::new(Spans::from(vec![
                        Span::styled(b.glyph.to_string(), b.style),
                        Span::raw(" ".to_string()),
                        Span::styled(name, name_style),
                        Span::raw(" ".repeat(pad)),
                        Span::styled(pct, theme::dim()),
                    ]));
                }
            }

            // Reserve the trailing note's width ("  {note}") so the marquee never
            // shoves it off the row.
            let note_w = b
                .note
                .as_deref()
                .map(|n| n.chars().count() + 2)
                .unwrap_or(0);
            // Only the hovered (highlighted) row marquees an over-long name; the
            // rest render normally and let the `List` clip them at the column
            // edge, keeping the list calm.
            let name = if is_selected {
                let budget = inner.saturating_sub(NAME_PREFIX + note_w);
                marquee(&game.get_name(), budget, marquee_tick)
            } else {
                game.get_name()
            };

            let mut spans: Vec<Span<'static>> = vec![
                Span::styled(b.glyph.to_string(), b.style),
                Span::raw(" ".to_string()),
                Span::styled(name, name_style),
            ];

            if let Some(note) = b.note {
                spans.push(Span::styled(format!("  {}", note), theme::dim()));
            }

            ListItem::new(Spans::from(spans))
        })
        .collect();

    List::new(items)
        .block(theme::panel(format!(
            "{} ({})",
            browser.filter.label(),
            browser.visible_len()
        )))
        .highlight_style(theme::selection(theme::ACCENT))
        .highlight_symbol("▶ ")
}

/// Scroll `name` like a marquee/roadsign banner when it is wider than `budget`
/// columns: the text streams left, separated from its wrapped-around start by a
/// short gap, looping forever. A name that already fits (or a zero budget) is
/// returned unchanged. `tick` is the animation step — each increment shifts the
/// window one column left.
fn marquee(name: &str, budget: usize, tick: usize) -> String {
    let chars: Vec<char> = name.chars().collect();
    if budget == 0 || chars.len() <= budget {
        return name.to_string();
    }

    // A blank-ish gap separates the tail of the name from its repeat so the loop
    // reads cleanly, like the dark stretch on a scrolling LED sign.
    const GAP: &str = "   •   ";
    let mut cycle = chars;
    cycle.extend(GAP.chars());
    let period = cycle.len();
    let start = tick % period;
    (0..budget).map(|i| cycle[(start + i) % period]).collect()
}

/// Truncate `name` to at most `budget` columns, marking a cut with an ellipsis.
/// Used for non-hovered downloading rows whose name must leave room for the
/// right-aligned percent.
fn clip(name: &str, budget: usize) -> String {
    let chars: Vec<char> = name.chars().collect();
    if budget == 0 {
        return String::new();
    }
    if chars.len() <= budget {
        return name.to_string();
    }
    let mut out: String = chars[..budget - 1].iter().collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::marquee;

    #[test]
    fn names_that_fit_are_unchanged() {
        assert_eq!(marquee("Doom", 10, 7), "Doom"); // shorter than the column
        assert_eq!(marquee("Doom", 4, 3), "Doom"); // exactly fills the column
        assert_eq!(marquee("Doom", 0, 1), "Doom"); // degenerate zero budget
    }

    #[test]
    fn overflowing_names_scroll_and_keep_the_column_width() {
        let name = "Counter-Strike"; // 14 chars, wider than the 6-col budget
        let budget = 6;

        let s0 = marquee(name, budget, 0);
        assert_eq!(s0.chars().count(), budget, "window is exactly the column wide");
        assert_eq!(s0, "Counte");

        // Each tick shifts the window one column to the left.
        assert_eq!(marquee(name, budget, 1), "ounter");

        // It loops: after a full period (name + the gap) it returns to the start.
        let period = name.chars().count() + "   •   ".chars().count();
        assert_eq!(marquee(name, budget, period), s0);
    }
}
