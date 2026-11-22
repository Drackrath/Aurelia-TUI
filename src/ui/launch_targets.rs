//! The launch-targets overlay ("Play as…", modal popup): every way an installed
//! game can be started — the native Linux build, or the Windows build under each
//! Steam mode — from `aurelia launch-targets`. Enter launches the highlighted one
//! for this run only, without changing the saved per-game config.

use tui::layout::Rect;
use tui::text::{Span, Spans};
use tui::widgets::Paragraph;

use crate::browse::Browser;
use crate::interface::launch::{self, RUNTIME_CONFIRM};
use crate::theme;
use crate::ui::{centered_rect, paneled_paragraph, scroll, selection_marker};

/// Overlay area within the frame.
pub fn area(frame: Rect) -> Rect {
    centered_rect(75, 60, frame)
}

/// Greedy word wrap to `width`.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split(' ') {
        let len = line.chars().count();
        if len > 0 && len + 1 + word.chars().count() > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// Pack hint items into lines.
fn pack(items: &[&str], width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for item in items {
        let len = line.chars().count();
        if len > 0 && len + 2 + item.chars().count() > width {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push_str("  ");
        }
        line.push_str(item);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// Build the overlay for `area` (scrolls).
pub fn launch_targets(browser: &Browser, area: Rect) -> Paragraph<'static> {
    let width = area.width.saturating_sub(2).max(1) as usize;
    let inner = area.height.saturating_sub(2) as usize;

    let mut footer: Vec<Spans<'static>> = vec![Spans::from("")];
    let dim = |text: String| Spans::from(Span::styled(text, theme::dim()));
    footer.push(Spans::from(Span::styled(
        play_uses(browser),
        theme::label(),
    )));
    if let Some(t) = browser.launch_targets.get(browser.launch_targets_index)
        && !t.available
    {
        let text = format!("Not available: {}", launch::missing_long(t));
        footer.extend(wrap(&text, width).into_iter().map(dim));
    }
    if let Some(progress) = browser.launch.runtime.progress_text() {
        footer.push(Spans::from(Span::styled(progress, theme::accent())));
    }
    if browser.launch.confirm_runtime {
        for line in wrap(RUNTIME_CONFIRM, width) {
            footer.push(Spans::from(Span::styled(line, theme::accent())));
        }
    } else {
        if !browser.launch_targets_status.is_empty() {
            footer.extend(
                wrap(&browser.launch_targets_status, width)
                    .into_iter()
                    .map(dim),
            );
        }
        footer.extend(pack(&hints(browser), width).into_iter().map(dim));
    }

    let len = browser.launch_targets.len();
    let list_height = inner.saturating_sub(footer.len()).max(1);
    let (start, end) = scroll::window(browser.launch_targets_index, len, list_height);

    let mut lines: Vec<Spans<'static>> = Vec::new();
    if len == 0 {
        lines.push(Spans::from(Span::styled(
            "No launch targets.".to_string(),
            theme::dim(),
        )));
    }
    for (i, target) in browser
        .launch_targets
        .iter()
        .enumerate()
        .take(end)
        .skip(start)
    {
        let selected = i == browser.launch_targets_index;
        let style = if selected {
            theme::selection(theme::ACCENT)
        } else if target.available {
            theme::value()
        } else {
            theme::dim()
        };
        let mut spans = vec![
            Span::styled(selection_marker(selected).to_string(), style),
            Span::styled(target.label(), style),
        ];
        if browser.launch.saved.as_deref() == Some(target.id.as_str()) {
            spans.push(Span::styled("  [your choice]".to_string(), theme::accent()));
        }
        if target.default {
            spans.push(Span::styled("  [auto]".to_string(), theme::accent()));
        }
        if !target.available {
            spans.push(Span::styled(
                format!("  ({})", launch::missing_short(target)),
                theme::dim(),
            ));
        }
        lines.push(Spans::from(spans));
    }
    lines.extend(footer);

    let title = if len > list_height {
        format!("Play as… ({}/{len})", browser.launch_targets_index + 1)
    } else {
        "Play as…".to_string()
    };
    paneled_paragraph(lines, title)
}

/// Which target plain Play uses.
fn play_uses(browser: &Browser) -> String {
    match browser.launch.saved.as_deref() {
        None => "Play uses: automatic".to_string(),
        Some(id) => match browser.launch_targets.iter().find(|t| t.id == id) {
            Some(t) if t.available => format!("Play uses: your choice, {}", t.label()),
            _ => "Play uses: automatic (your choice is unavailable)".to_string(),
        },
    }
}

/// Key hints for the highlighted row.
fn hints(browser: &Browser) -> Vec<&'static str> {
    let mut hints = vec!["[Enter] launch once", "[d] use for Play", "[a] automatic"];
    if browser.launch_target_needs_runtime() && !browser.launch.runtime.is_running() {
        hints.push("[i] install Steam runtime");
    }
    hints.push("[Esc] close");
    hints
}
