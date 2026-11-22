//! Play settings rows for two overlays.

use tui::layout::Rect;
use tui::text::{Span, Spans};
use tui::widgets::{Paragraph, Wrap};

use crate::browse::{Browser, ROW_LAUNCH_OPTIONS, ROW_PREFIX};
use crate::theme;
use crate::ui::scroll::window;
use crate::ui::selection_marker;

/// Value text, or the input with caret.
fn editable(browser: &Browser, row: usize, value: String) -> String {
    match &browser.game_config_input {
        Some(input) if browser.game_config_index == row => format!("{input}_"),
        _ => value,
    }
}

/// Launch options and prefix rows.
pub fn game_config_rows(browser: &Browser) -> Vec<(&'static str, String)> {
    let launch = browser.game_config.as_ref().map(|c| &c.launch);
    let options = match launch.map(|l| l.launch_options.as_str()) {
        Some(line) if !line.is_empty() => line.to_string(),
        _ => "(none)".to_string(),
    };
    let prefix = launch
        .and_then(|l| l.prefix_dir.clone())
        .unwrap_or_else(|| "(Steam compatdata)".to_string());
    vec![
        (
            "Launch options",
            editable(browser, ROW_LAUNCH_OPTIONS, options),
        ),
        ("Wine prefix", editable(browser, ROW_PREFIX, prefix)),
    ]
}

/// The Wine prefix launches use.
pub fn prefix_in_use(browser: &Browser) -> Option<String> {
    let launch = &browser.game_config.as_ref()?.launch;
    let path = launch.wine_prefix.clone()?;
    let custom = if launch.prefix_dir.is_some() {
        " (custom)"
    } else {
        ""
    };
    Some(format!("Prefix in use: {path}{custom}"))
}

/// Prefix line, path shortened to `width`.
pub fn prefix_line(browser: &Browser, width: usize) -> Option<String> {
    let full = prefix_in_use(browser)?;
    let label = "Prefix in use: ";
    let rest = &full[label.len()..];
    let room = width.saturating_sub(label.len());
    Some(format!("{label}{}", tail_fit(rest, room)))
}

/// Hint parts while editing text.
pub fn edit_hint(browser: &Browser) -> Option<Vec<&'static str>> {
    browser.game_config_input.as_ref()?;
    let example = if browser.game_config_index == ROW_PREFIX {
        "~ expands"
    } else {
        "e.g. VAR=1 %command% -arg"
    };
    Some(vec![
        "[Enter] save",
        "[Esc] cancel",
        "empty clears",
        example,
    ])
}

/// Wrap hint parts to `width`.
pub fn wrap_hints(parts: &[&str], width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    for part in parts {
        let extra = if cur.is_empty() { 0 } else { 3 };
        if !cur.is_empty() && cur.chars().count() + extra + part.chars().count() > width {
            lines.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push_str(" · ");
        }
        cur.push_str(part);
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

/// Keep the tail within `width`.
pub fn tail_fit(text: &str, width: usize) -> String {
    let n = text.chars().count();
    if n <= width || width == 0 {
        return text.to_string();
    }
    let tail: String = text.chars().skip(n - width + 1).collect();
    format!("…{tail}")
}

/// The choice picker over Game settings.
pub fn picker(browser: &Browser, area: Rect) -> Paragraph<'static> {
    let Some(p) = &browser.play.picker else {
        return crate::ui::paneled_paragraph(Vec::<Spans>::new(), String::new());
    };
    let inner_w = area.width.saturating_sub(2) as usize;
    let hints = wrap_hints(&["[Enter] apply", "[Esc] cancel"], inner_w);
    let rows_h = (area.height.saturating_sub(2) as usize).saturating_sub(hints.len());
    let (start, end) = window(p.index, p.options.len(), rows_h);
    let mut lines: Vec<Spans<'static>> = Vec::new();
    for i in start..end {
        let (label, _) = &p.options[i];
        let selected = i == p.index;
        let style = if selected {
            theme::selection(theme::ACCENT)
        } else {
            theme::value()
        };
        let mut spans = vec![
            Span::styled(selection_marker(selected).to_string(), style),
            Span::styled(label.clone(), style),
        ];
        if i == p.current {
            spans.push(Span::styled("  [current]".to_string(), theme::dim()));
        }
        lines.push(Spans::from(spans));
    }
    for h in hints {
        lines.push(Spans::from(Span::styled(h, theme::dim())));
    }
    crate::ui::paneled_paragraph(lines, p.title.clone())
}

/// The play confirm popup, if pending.
pub fn confirm_prompt(browser: &Browser) -> Option<Paragraph<'static>> {
    let prompt = browser.play_confirm_prompt()?;
    Some(crate::ui::confirm::confirm(prompt).wrap(Wrap { trim: true }))
}

/// Global Settings rows from `config show`.
pub fn settings_rows(browser: &Browser) -> Vec<(&'static str, String)> {
    let Some(info) = &browser.config_info else {
        return Vec::new();
    };
    let on_off = |on: bool| if on { "on" } else { "off" }.to_string();
    vec![
        (
            "Shader pre-caching",
            if info.play.shaders_auto() {
                "auto"
            } else {
                "off"
            }
            .to_string(),
        ),
        (
            "Anti-cheat auto-install",
            on_off(info.play.anticheat_auto_install),
        ),
    ]
}
