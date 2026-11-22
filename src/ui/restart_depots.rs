//! Restart prompts and depot file browser.

use pretty_bytes::converter::convert;

use tui::Frame;
use tui::backend::Backend;
use tui::layout::{Alignment, Constraint, Direction, Layout};
use tui::style::{Modifier, Style};
use tui::text::{Span, Spans, Text};
use tui::widgets::{BorderType, Borders, Clear, Paragraph};

use crate::browse::Browser;
use crate::interface::depot_files::{DepotFileJson, ManifestSel};
use crate::restart_depots::{DownloadState, FileBrowser, FileInput, RESTART_QUESTION};
use crate::theme;

/// Draw our open overlays.
pub fn draw<B: Backend>(frame: &mut Frame<B>, browser: &Browser) {
    let state = &browser.restart_depots;
    if let Some(files) = &state.files {
        let area = crate::ui::centered_rect(80, 85, frame.size());
        frame.render_widget(Clear, area);
        let rows = area.height.saturating_sub(7) as usize;
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(5)])
            .split(area);
        frame.render_widget(file_list(files, rows.max(1)), chunks[0]);
        frame.render_widget(file_footer(files), chunks[1]);
    }
    if state.restart_prompt.is_some() {
        draw_question(frame, RESTART_QUESTION.to_string(), String::new());
    }
    if let Some(msg) = &state.force_push_prompt {
        draw_question(frame, "Push anyway? (y/n)".to_string(), msg.clone());
    }
}

fn draw_question<B: Backend>(frame: &mut Frame<B>, question: String, detail: String) {
    let area = crate::ui::centered_rect(50, 25, frame.size());
    frame.render_widget(Clear, area);
    frame.render_widget(question_box(question, detail), area);
}

/// WARN-bordered y/n popup.
pub fn question_box(question: String, detail: String) -> Paragraph<'static> {
    let mut lines = vec![Spans::from(Span::styled(
        question,
        Style::default().fg(theme::TEXT),
    ))];
    if !detail.is_empty() {
        lines.push(Spans::from(""));
        lines.push(Spans::from(Span::styled(detail, theme::dim())));
    }
    lines.push(Spans::from(""));
    lines.push(Spans::from(vec![
        Span::styled("[y]", theme::key()),
        Span::styled(" confirm   ", theme::dim()),
        Span::styled("[n]", theme::key()),
        Span::styled(" cancel", theme::dim()),
    ]));
    let block = tui::widgets::Block::default()
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
        .wrap(tui::widgets::Wrap { trim: true })
}

/// Size column text for one entry.
pub fn entry_size(entry: &DepotFileJson) -> String {
    match entry.kind.as_str() {
        "directory" => "<dir>".to_string(),
        "symlink" => "<link>".to_string(),
        _ => convert(entry.size as f64),
    }
}

/// Path text with kind marker.
pub fn entry_label(entry: &DepotFileJson) -> String {
    match entry.kind.as_str() {
        "directory" => format!("{}/", entry.path),
        "symlink" => match &entry.link_target {
            Some(target) => format!("{} -> {target}", entry.path),
            None => entry.path.clone(),
        },
        _ if entry.executable => format!("{}*", entry.path),
        _ => entry.path.clone(),
    }
}

/// The scrolling list of manifest entries.
pub fn file_list(files: &FileBrowser, rows: usize) -> Paragraph<'static> {
    let manifest = files
        .listing
        .as_ref()
        .map(|l| format!(", manifest {}", l.manifest_id))
        .unwrap_or_default();
    let branch = match &files.sel {
        ManifestSel::Branch(b) => format!(", branch {b}"),
        _ => String::new(),
    };
    let title = format!("Depot {}{manifest}{branch}", files.depot_label);

    let lines: Vec<Spans<'static>> = match &files.listing {
        None => vec![Spans::from(Span::styled(
            if files.error.is_empty() {
                "Loading...".to_string()
            } else {
                files.error.clone()
            },
            theme::dim(),
        ))],
        Some(listing) if listing.files.is_empty() => vec![Spans::from(Span::styled(
            if files.filter.is_empty() {
                "The manifest is empty.".to_string()
            } else {
                "No entries match the filter.".to_string()
            },
            theme::dim(),
        ))],
        Some(listing) => {
            let top = files.index.saturating_sub(rows.saturating_sub(1));
            listing
                .files
                .iter()
                .enumerate()
                .skip(top)
                .take(rows)
                .map(|(i, entry)| {
                    let selected = i == files.index;
                    let style = if selected {
                        theme::selection(theme::ACCENT)
                    } else if entry.is_dir() {
                        theme::dim()
                    } else {
                        theme::value()
                    };
                    Spans::from(vec![
                        Span::styled(crate::ui::selection_marker(selected).to_string(), style),
                        Span::styled(format!("{:>10}  ", entry_size(entry)), theme::dim()),
                        Span::styled(entry_label(entry), style),
                    ])
                })
                .collect()
        }
    };
    crate::ui::paneled_paragraph(lines, title)
}

/// Input line, status and key hints.
pub fn file_footer(files: &FileBrowser) -> Paragraph<'static> {
    let mut lines = Vec::new();
    match &files.input {
        Some((field, text)) => {
            let label = match field {
                FileInput::Filter => "Filter (glob, * ?): ",
                FileInput::Manifest => "Branch or manifest id: ",
                FileInput::Target => "Save to folder: ",
            };
            lines.push(Spans::from(vec![
                Span::styled(label.to_string(), Style::default().fg(theme::TEXT)),
                Span::styled(text.clone(), theme::accent()),
                Span::styled("_".to_string(), theme::accent()),
            ]));
        }
        None => {
            let summary = files
                .listing
                .as_ref()
                .map(|l| {
                    let count = l.files.iter().filter(|f| f.is_file()).count();
                    format!("{count} files, {}", convert(l.total_size as f64))
                })
                .unwrap_or_default();
            let filter = if files.filter.is_empty() {
                String::new()
            } else {
                format!("  filter: {}", files.filter)
            };
            lines.push(Spans::from(Span::styled(
                format!("{summary}{filter}"),
                theme::dim(),
            )));
        }
    }
    let status = match files.download_state() {
        DownloadState::Idle => String::new(),
        DownloadState::Running(path) => format!("Downloading {path}..."),
        DownloadState::Done(outcome) => outcome.line(),
    };
    lines.push(Spans::from(Span::styled(
        status,
        Style::default().fg(theme::WARN),
    )));
    lines.push(Spans::from(Span::styled(
        "[Enter/d] download  [/] filter  [b] branch/manifest  [Esc] back".to_string(),
        theme::dim(),
    )));
    Paragraph::new(Text::from(lines))
        .block(theme::panel("Files".to_string()))
        .style(theme::base())
        .wrap(tui::widgets::Wrap { trim: true })
}

/// DLC row tag when queued.
pub fn pending_dlc_label(app_id: u32) -> Option<String> {
    crate::interface::steam_restart::pending_for(app_id).map(|change| {
        let verb = if change.enable { "enable" } else { "disable" };
        format!("  ({verb} pending until next launch)")
    })
}
