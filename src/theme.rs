//! Steam-inspired colour palette and shared widget styles.
//!
//! Everything visual flows through here so the whole TUI stays consistent: the
//! Steam dark look is navy panels on a near-black canvas, a light-blue accent,
//! soft blue borders, and light blue-grey text with bright white headings.

use tui::style::{Color, Modifier, Style};
use tui::text::{Span, Spans};
use tui::widgets::{Block, BorderType, Borders};

// --- Palette (Steam dark theme) ---

/// Panel background (Steam "store" navy).
pub const BG: Color = Color::Rgb(0x1b, 0x28, 0x38);
/// Canvas/app background behind the panels (darker navy).
pub const BG_DARK: Color = Color::Rgb(0x10, 0x16, 0x1d);
/// Raised/selected surface (lighter slate blue).
pub const BG_RAISED: Color = Color::Rgb(0x2a, 0x47, 0x5e);
/// Primary accent — Steam's signature light blue.
pub const ACCENT: Color = Color::Rgb(0x66, 0xc0, 0xf4);
/// Brighter accent for emphasis / links.
pub const ACCENT_BRIGHT: Color = Color::Rgb(0x1a, 0x9f, 0xff);
/// Subtle panel border.
pub const BORDER: Color = Color::Rgb(0x3d, 0x5a, 0x73);
/// Primary body text.
pub const TEXT: Color = Color::Rgb(0xc6, 0xd4, 0xdf);
/// Secondary / muted text.
pub const TEXT_DIM: Color = Color::Rgb(0x76, 0x8f, 0xa3);
/// Bright heading text.
pub const HEADER: Color = Color::Rgb(0xff, 0xff, 0xff);
/// Installed / positive (Steam green).
pub const ONLINE: Color = Color::Rgb(0x8f, 0xd4, 0x50);
/// Update available / warning (amber).
pub const WARN: Color = Color::Rgb(0xf2, 0xc1, 0x4e);

// --- Styles ---

/// Text on a panel background.
pub fn base() -> Style {
    Style::default().fg(TEXT).bg(BG)
}

/// Full-frame canvas behind the panels.
pub fn canvas() -> Style {
    Style::default().fg(TEXT).bg(BG_DARK)
}

/// Panel/window title (accent, bold).
pub fn title_style() -> Style {
    Style::default().fg(ACCENT).bg(BG).add_modifier(Modifier::BOLD)
}

/// Detail-table label (accent).
pub fn label() -> Style {
    Style::default().fg(ACCENT)
}

/// Detail-table value (body text).
pub fn value() -> Style {
    Style::default().fg(TEXT)
}

/// List selection — a bright Steam-blue bar. `highlight` is the configurable
/// accent (defaults to [`ACCENT`]).
pub fn selection(highlight: Color) -> Style {
    Style::default()
        .fg(BG_DARK)
        .bg(highlight)
        .add_modifier(Modifier::BOLD)
}

/// Installed game (normal, readable).
pub fn item_installed() -> Style {
    Style::default().fg(TEXT)
}

/// Game with an update available.
pub fn item_update() -> Style {
    Style::default().fg(WARN)
}

/// Uninstalled / unavailable game (muted).
pub fn item_muted() -> Style {
    Style::default().fg(TEXT_DIM)
}

/// Failed operation.
pub fn item_failed() -> Style {
    Style::default().fg(WARN).add_modifier(Modifier::BOLD)
}

/// Muted secondary text (counts, hints, inactive items).
pub fn dim() -> Style {
    Style::default().fg(TEXT_DIM)
}

/// Accent text (active values, highlights) without a background.
pub fn accent() -> Style {
    Style::default().fg(ACCENT)
}

/// Bright key-cap / emphasis text.
pub fn key() -> Style {
    Style::default().fg(ACCENT_BRIGHT).add_modifier(Modifier::BOLD)
}

/// Active filter tab (dark text on a bright accent chip).
pub fn tab_active() -> Style {
    Style::default()
        .fg(BG_DARK)
        .bg(ACCENT)
        .add_modifier(Modifier::BOLD)
}

/// Inactive filter tab.
pub fn tab_inactive() -> Style {
    Style::default().fg(TEXT_DIM).bg(BG)
}

/// A standard Steam-styled panel: rounded soft-blue border, accent title, navy
/// fill. Title is owned so the returned block is `'static`.
pub fn panel(title: String) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(BORDER).bg(BG))
        .title(Spans::from(Span::styled(title, title_style())))
        .style(base())
}
