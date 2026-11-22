//! Install picker rows, language picker, uninstall prompt.

use pretty_bytes::converter::convert;
use tui::Frame;
use tui::backend::Backend;
use tui::layout::Rect;
use tui::style::{Modifier, Style};
use tui::text::{Span, Spans, Text};
use tui::widgets::{Block, BorderType, Borders, Clear, Paragraph};

use crate::browse::Browser;
use crate::browse::install_options::{InstallPickerOptions, has_wine_prefix};
use crate::interface::game::Game;
use crate::interface::install_options::{PlatformChoice, language_label};
use crate::theme;
use crate::util::stateful::Named;

/// Language cycle keys, as in help.
pub const LANGUAGE_KEYS: &str = "←/→ or h/l";

/// Platform, language and size rows.
pub fn picker_rows(opts: &InstallPickerOptions, disk: Option<u64>) -> Vec<Spans<'static>> {
    let mut lines = Vec::new();
    let current = opts.current();
    let sizing = opts.sizing();

    let mut platform = opts.platform.label().to_string();
    if opts.platform == PlatformChoice::Auto
        && let Some(dry) = current.filter(|d| !d.platform.is_empty())
    {
        platform = format!("auto ({})", dry.platform);
    }
    if opts.choices().len() > 1 {
        platform.push_str(" ▸");
    }
    let mut spans = vec![
        Span::styled("Platform  ", theme::dim()),
        Span::styled(platform, theme::value()),
    ];
    if opts.choices().len() > 1 {
        spans.push(Span::styled("  [p]".to_string(), theme::key()));
    }
    if opts.blocked() {
        spans.push(Span::styled(
            format!("  ✗ no {} build", opts.platform.label()),
            Style::default().fg(theme::WARN),
        ));
    }
    lines.push(Spans::from(spans));

    if !opts.languages().is_empty() {
        let lang = opts
            .language
            .as_deref()
            .map(language_label)
            .unwrap_or_default();
        lines.push(Spans::from(vec![
            Span::styled("Language  ", theme::dim()),
            Span::styled(format!("◂ {lang} ▸"), theme::value()),
            Span::styled(format!("  [{LANGUAGE_KEYS}]"), theme::key()),
        ]));
    }

    let size = |bytes: Option<u64>| match bytes {
        _ if sizing => "Sizing…".to_string(),
        Some(b) if b > 0 => convert(b as f64),
        _ => "unknown".to_string(),
    };
    lines.push(Spans::from(vec![
        Span::styled("Download  ", theme::dim()),
        Span::styled(size(current.map(|d| d.download_size)), theme::value()),
        Span::styled("  ·  on disk ", theme::dim()),
        Span::styled(size(disk), theme::value()),
    ]));
    lines.push(Spans::from(""));
    lines
}

/// Install picker key hints.
pub fn install_hints(languages: bool) -> Vec<(&'static str, &'static str)> {
    let mut hints = vec![("↑/↓", "library"), ("p", "platform")];
    if languages {
        hints.push((LANGUAGE_KEYS, "language"));
    }
    hints.extend([("Enter", "install"), ("Esc", "cancel")]);
    hints
}

/// Wrap `[key] label` hints to `width`.
pub fn hint_lines(hints: &[(&str, &str)], width: u16) -> Vec<Spans<'static>> {
    let width = width.max(1) as usize;
    let mut lines = Vec::new();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut used = 0usize;
    for (key, label) in hints {
        let key = format!("[{key}]");
        let label = format!(" {label}");
        let len = key.chars().count() + label.chars().count();
        let gap = if used == 0 { 0 } else { 2 };
        if used > 0 && used + gap + len > width {
            lines.push(Spans::from(std::mem::take(&mut spans)));
            used = 0;
        }
        if used > 0 {
            spans.push(Span::raw("  ".to_string()));
            used += 2;
        }
        spans.push(Span::styled(key, theme::key()));
        spans.push(Span::styled(label, theme::dim()));
        used += len;
    }
    if !spans.is_empty() {
        lines.push(Spans::from(spans));
    }
    lines
}

/// Centered rect of fixed size.
pub fn centered(full: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(full.width);
    let height = height.min(full.height);
    Rect::new(
        full.x + (full.width - width) / 2,
        full.y + (full.height - height) / 2,
        width,
        height,
    )
}

/// Overlay width for a frame.
pub fn overlay_width(full: Rect, max: u16) -> u16 {
    full.width.saturating_sub(4).min(max)
}

/// Draw body, then hints at bottom.
pub fn draw_panel<B: Backend>(
    frame: &mut Frame<B>,
    area: Rect,
    block: Block<'static>,
    body: Vec<Spans<'static>>,
    footer: Vec<Spans<'static>>,
) {
    frame.render_widget(Clear, area);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let foot_h = (footer.len() as u16).min(inner.height);
    let body_h = inner.height - foot_h;
    let body_area = Rect::new(inner.x, inner.y, inner.width, body_h);
    let foot_area = Rect::new(inner.x, inner.y + body_h, inner.width, foot_h);
    frame.render_widget(
        Paragraph::new(Text::from(body)).style(theme::base()),
        body_area,
    );
    frame.render_widget(
        Paragraph::new(Text::from(footer)).style(theme::base()),
        foot_area,
    );
}

/// Draw the change-language overlay.
pub fn draw_language_picker<B: Backend>(frame: &mut Frame<B>, browser: &Browser) {
    let Some(picker) = browser.language_picker.as_ref() else {
        return;
    };
    let full = frame.size();
    let width = overlay_width(full, 60);
    let hints = hint_lines(
        &[("↑/↓", "choose"), ("Enter", "switch"), ("Esc", "cancel")],
        width.saturating_sub(2),
    );
    let mut head: Vec<Spans<'static>> = Vec::new();
    if picker.loading {
        head.push(Spans::from(Span::styled(
            "Loading languages…".to_string(),
            theme::dim(),
        )));
    } else if picker.current.is_none() {
        head.push(Spans::from(Span::styled(
            "Current language unknown.".to_string(),
            theme::dim(),
        )));
    }
    let rows = picker.languages.len();
    let wanted = (head.len() + rows + hints.len() + 2) as u16;
    let height = wanted.min(full.height.saturating_sub(2));
    let area = centered(full, width, height);
    let list_h = (height as usize).saturating_sub(2 + hints.len() + head.len());
    let (start, end) = crate::ui::scroll::window(picker.index, rows, list_h);
    let mut body = head;
    for i in start..end {
        let lang = &picker.languages[i];
        let selected = i == picker.index;
        let style = if selected {
            theme::selection(theme::ACCENT)
        } else {
            theme::value()
        };
        let mut spans = vec![
            Span::styled(crate::ui::selection_marker(selected).to_string(), style),
            Span::styled(language_label(lang), style),
        ];
        if picker.is_current(lang) {
            spans.push(Span::styled("  ● installed".to_string(), theme::dim()));
        }
        body.push(Spans::from(spans));
    }
    let block = theme::panel(format!("Language of {}", picker.game_name));
    draw_panel(frame, area, block, body, hints);
}

/// Draw the uninstall prompt.
pub fn draw_uninstall<B: Backend>(frame: &mut Frame<B>, browser: &Browser, game: &Game) {
    let full = frame.size();
    let width = overlay_width(full, 64);
    let prefix = has_wine_prefix(game);
    let mut hints = vec![("y", "uninstall")];
    if prefix {
        hints.push(("p", "toggle prefix"));
    }
    hints.push(("any key", "cancel"));
    let footer = hint_lines(&hints, width.saturating_sub(2));
    let mut body = vec![
        Spans::from(Span::styled(
            format!("Uninstall {}?", game.get_name()),
            Style::default().fg(theme::TEXT),
        )),
        Spans::from(""),
    ];
    if prefix {
        let on = browser.uninstall_delete_prefix;
        let (mark, style) = if on {
            ("[x]", Style::default().fg(theme::WARN))
        } else {
            ("[ ]", theme::dim())
        };
        body.push(Spans::from(Span::styled(
            format!("{mark} also delete Wine prefix"),
            style,
        )));
        body.push(Spans::from(Span::styled(
            "    (saves inside it are lost)".to_string(),
            style,
        )));
    }
    let height = ((body.len() + footer.len() + 2) as u16).min(full.height);
    let area = centered(full, width, height);
    draw_panel(frame, area, confirm_block(), body, footer);
}

fn confirm_block() -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::WARN).bg(theme::BG))
        .title(Spans::from(Span::styled(
            "Confirm".to_string(),
            Style::default()
                .fg(theme::WARN)
                .bg(theme::BG)
                .add_modifier(Modifier::BOLD),
        )))
        .style(theme::base())
}
