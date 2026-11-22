//! Scrollable Aurelia log overlay.

use crossterm::event::KeyCode;
use tui::text::{Span, Spans};
use tui::widgets::Paragraph;

use crate::interface::aurelia_log;
use crate::theme;
use crate::ui::paneled_paragraph;

/// Lines scrolled per page.
const PAGE: usize = 20;

/// Overlay state: lines and scroll.
#[derive(Debug, Default)]
pub struct LogView {
    pub open: bool,
    pub lines: Vec<String>,
    /// Lines hidden below the view.
    pub from_bottom: usize,
}

impl LogView {
    /// Open at the newest line.
    pub fn open(&mut self) {
        self.lines = aurelia_log::read_lines();
        self.from_bottom = 0;
        self.open = true;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.lines.clear();
    }

    /// Scroll up (older) by `n`.
    pub fn up(&mut self, n: usize) {
        let max = self.lines.len().saturating_sub(1);
        self.from_bottom = (self.from_bottom + n).min(max);
    }

    /// Scroll down (newer) by `n`.
    pub fn down(&mut self, n: usize) {
        self.from_bottom = self.from_bottom.saturating_sub(n);
    }

    /// The `height` lines to show.
    pub fn visible(&self, height: usize) -> &[String] {
        let end = self.lines.len().saturating_sub(self.from_bottom);
        let start = end.saturating_sub(height);
        &self.lines[start..end]
    }

    /// Handle a key; false means unhandled.
    pub fn handle_key(&mut self, key: KeyCode) -> bool {
        match key {
            KeyCode::Esc | KeyCode::Char('q') => self.close(),
            KeyCode::Up | KeyCode::Char('k') => self.up(1),
            KeyCode::Down | KeyCode::Char('j') => self.down(1),
            KeyCode::PageUp => self.up(PAGE),
            KeyCode::PageDown => self.down(PAGE),
            KeyCode::Char('g') | KeyCode::Home => self.up(self.lines.len()),
            KeyCode::Char('G') | KeyCode::End => self.from_bottom = 0,
            KeyCode::Char('r') => {
                let keep = self.from_bottom;
                self.open();
                self.up(keep);
            }
            _ => return false,
        }
        true
    }
}

/// Render the overlay for `height` rows.
pub fn render(view: &LogView, height: u16) -> Paragraph<'static> {
    // Panel border plus hint line.
    let rows = usize::from(height).saturating_sub(3);
    let mut lines: Vec<Spans<'static>> = vec![Spans::from(Span::styled(
        "j/k scroll · PgUp/PgDn page · g/G top/end · r reload · Esc close".to_string(),
        theme::dim(),
    ))];
    if view.lines.is_empty() {
        lines.push(Spans::from(Span::styled(
            "No Aurelia output logged yet.".to_string(),
            theme::dim(),
        )));
    } else {
        for line in view.visible(rows) {
            lines.push(Spans::from(Span::styled(line.clone(), theme::value())));
        }
    }
    let path = aurelia_log::log_path()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    paneled_paragraph(lines, format!("Aurelia log — {path}"))
}
