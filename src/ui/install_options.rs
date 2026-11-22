//! Install picker rows, language picker, uninstall prompt.

use pretty_bytes::converter::convert;
use tui::layout::Alignment;
use tui::style::{Modifier, Style};
use tui::text::{Span, Spans, Text};
use tui::widgets::{Block, BorderType, Borders, List, ListItem, Paragraph};

use crate::browse::Browser;
use crate::browse::install_options::InstallPickerOptions;
use crate::interface::install_options::{PlatformChoice, language_label};
use crate::theme;

/// Platform and language picker rows.
pub fn picker_rows(opts: &InstallPickerOptions) -> Vec<Spans<'static>> {
    let mut lines = Vec::new();
    let current = opts.current();

    let mut platform = format!("{} ▸", opts.platform.label());
    if opts.platform == PlatformChoice::Auto
        && let Some(dry) = current.filter(|d| !d.platform.is_empty())
    {
        platform = format!("auto ({}) ▸", dry.platform);
    }
    let mut spans = vec![
        Span::styled("Platform  ", theme::dim()),
        Span::styled(platform, theme::value()),
        Span::styled("  [p]".to_string(), theme::key()),
    ];
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
            Span::styled("  [←/→]".to_string(), theme::key()),
        ]));
    }

    if let Some(dry) = current.filter(|d| d.download_size > 0) {
        lines.push(Spans::from(vec![
            Span::styled("Download  ", theme::dim()),
            Span::styled(convert(dry.download_size as f64), theme::value()),
        ]));
    }
    lines.push(Spans::from(""));
    lines
}

/// The change-language overlay.
pub fn language_picker(browser: &Browser) -> List<'static> {
    let Some(picker) = browser.language_picker.as_ref() else {
        return List::new(crate::ui::empty_list_rows("No languages."));
    };
    let items: Vec<ListItem<'static>> = picker
        .languages
        .iter()
        .enumerate()
        .map(|(i, lang)| {
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
            ListItem::new(Spans::from(spans))
        })
        .collect();
    List::new(items)
        .block(theme::panel(format!(
            "Language of {}  ([Enter] switch · [Esc] cancel)",
            picker.game_name
        )))
        .style(theme::base())
}

/// Uninstall prompt with prefix choice.
pub fn confirm_uninstall_game(game_name: &str) -> Paragraph<'static> {
    let hint = Style::default().fg(theme::TEXT_DIM);
    let lines = vec![
        Spans::from(Span::styled(
            format!("Uninstall {}?", game_name),
            Style::default().fg(theme::TEXT),
        )),
        Spans::from(""),
        Spans::from(vec![
            Span::styled("[y]", theme::key()),
            Span::styled(" keep Wine prefix   ", hint),
            Span::styled("[Y]", theme::key()),
            Span::styled(" delete prefix", hint),
        ]),
        Spans::from(vec![
            Span::styled("[n]", theme::key()),
            Span::styled(" cancel", hint),
        ]),
    ];
    let block = Block::default()
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
        .style(theme::base());
    Paragraph::new(Text::from(lines))
        .block(block)
        .style(theme::base())
        .alignment(Alignment::Center)
}
