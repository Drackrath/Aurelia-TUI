//! Restart prompts and depot file browser.

use pretty_bytes::converter::convert;

use tui::Frame;
use tui::backend::Backend;
use tui::layout::{Alignment, Constraint, Direction, Layout, Rect};
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
        draw_files(frame, files, overlay_area(frame.size()));
    }
    if let Some(request) = state.files.as_ref().and_then(|f| f.confirm.as_ref()) {
        let size = convert(request.size as f64);
        let question = format!(
            "Download {} ({size}) to {}? (y/n)",
            request.path, request.out_dir
        );
        draw_question(frame, question, String::new());
    }
    if state.restart_prompt.is_some() {
        draw_question(frame, RESTART_QUESTION.to_string(), String::new());
    }
    if let Some(msg) = &state.force_push_prompt {
        draw_question(frame, "Push anyway? (y/n)".to_string(), msg.clone());
    }
}

/// Tab bar rows above content.
const TOP_ROWS: u16 = 3;
/// Status and key bar rows.
const BOTTOM_ROWS: u16 = 2;

/// Overlay rect inside content area.
pub fn overlay_area(frame: Rect) -> Rect {
    let top = TOP_ROWS.min(frame.height);
    let height = frame.height.saturating_sub(top + BOTTOM_ROWS);
    let content = Rect {
        y: frame.y + top,
        height,
        ..frame
    };
    crate::ui::centered_rect(90, 100, content)
}

/// Draw the file list and footer.
pub fn draw_files<B: Backend>(frame: &mut Frame<B>, files: &FileBrowser, area: Rect) {
    frame.render_widget(Clear, area);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(FOOTER_ROWS)])
        .split(area);
    let rows = chunks[0].height.saturating_sub(2) as usize;
    let width = area.width.saturating_sub(2) as usize;
    frame.render_widget(file_list(files, rows.max(1), width), chunks[0]);
    frame.render_widget(file_footer(files, width), chunks[1]);
}

/// Rect sized to fit a question.
pub fn question_area(frame: Rect, question: &str, detail: &str) -> Rect {
    let max_w = frame.width.saturating_sub(4).max(10);
    let longest = question.chars().count().max(detail.chars().count()) as u16;
    let width = (longest + 4).clamp(40.min(max_w), max_w);
    let inner = width.saturating_sub(2).max(1) as usize;
    let wrapped = |t: &str| t.chars().count().div_ceil(inner).max(1) as u16;
    let mut height = wrapped(question) + 2 + 2;
    if !detail.is_empty() {
        height += wrapped(detail) + 1;
    }
    let height = height.min(frame.height);
    Rect {
        x: frame.x + (frame.width.saturating_sub(width)) / 2,
        y: frame.y + (frame.height.saturating_sub(height)) / 2,
        width: width.min(frame.width),
        height,
    }
}

fn draw_question<B: Backend>(frame: &mut Frame<B>, question: String, detail: String) {
    let area = question_area(frame.size(), &question, &detail);
    frame.render_widget(Clear, area);
    frame.render_widget(question_box(question, detail), area);
}

/// Fit to `width`, keeping tail.
pub fn fit_tail(text: &str, width: usize) -> String {
    let n = text.chars().count();
    if n <= width || width < 2 {
        return text.to_string();
    }
    let tail: String = text.chars().skip(n - (width - 1)).collect();
    format!("\u{2026}{tail}")
}

/// Fit to `width`, keeping head.
pub fn fit_head(text: &str, width: usize) -> String {
    if text.chars().count() <= width || width < 2 {
        return text.to_string();
    }
    let head: String = text.chars().take(width - 1).collect();
    format!("{head}\u{2026}")
}

/// Footer rows, borders included.
pub const FOOTER_ROWS: u16 = 6;

/// Footer key hints.
pub const FILE_HINTS: &str =
    "[d] download [Enter] info [/] filter [b] branch [g/G] ends [Esc] back";

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
pub fn file_list(files: &FileBrowser, rows: usize, width: usize) -> Paragraph<'static> {
    let manifest = files
        .listing
        .as_ref()
        .map(|l| format!(" \u{b7} manifest {}", l.manifest_id))
        .unwrap_or_default();
    let branch = match &files.sel {
        ManifestSel::Branch(b) => format!(" \u{b7} branch {b}"),
        _ => String::new(),
    };
    let suffix = format!("{manifest}{branch}");
    let room = width.saturating_sub(suffix.chars().count() + 2);
    let title = format!("{}{suffix}", fit_head(&files.depot_label, room));

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
            let (start, end) = crate::ui::scroll::window(files.index, listing.files.len(), rows);
            listing
                .files
                .iter()
                .enumerate()
                .skip(start)
                .take(end - start)
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
pub fn file_footer(files: &FileBrowser, width: usize) -> Paragraph<'static> {
    let mut lines = Vec::new();
    match &files.input {
        Some((field, text)) => {
            let label = match field {
                FileInput::Filter => "Filter (glob, * ?): ",
                FileInput::Manifest => "Branch or manifest id: ",
                FileInput::Target => "Save to folder ([Enter] ok): ",
            };
            let room = width.saturating_sub(label.chars().count() + 1);
            lines.push(Spans::from(vec![
                Span::styled(label.to_string(), Style::default().fg(theme::TEXT)),
                Span::styled(fit_tail(text, room), theme::accent()),
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
                fit_head(&format!("{summary}{filter}"), width),
                theme::dim(),
            )));
        }
    }
    let details = match files.selected() {
        Some(entry) if files.details => entry_details(entry),
        _ => String::new(),
    };
    lines.push(Spans::from(Span::styled(
        fit_head(&details, width),
        theme::value(),
    )));
    let status = match files.download_state() {
        DownloadState::Idle => String::new(),
        DownloadState::Running(path) => format!("Downloading {path}..."),
        DownloadState::Done(outcome) => outcome.line(),
    };
    lines.push(Spans::from(Span::styled(
        fit_tail(&status, width),
        Style::default().fg(theme::WARN),
    )));
    lines.push(Spans::from(Span::styled(
        fit_head(FILE_HINTS, width),
        theme::dim(),
    )));
    Paragraph::new(Text::from(lines))
        .block(theme::panel("Files".to_string()))
        .style(theme::base())
}

/// One-line details for an entry.
pub fn entry_details(entry: &DepotFileJson) -> String {
    let mut parts = vec![entry.path.clone(), entry.kind.clone()];
    if entry.is_file() {
        parts.push(convert(entry.size as f64));
        parts.push(format!("{} chunk(s)", entry.chunks));
    }
    if entry.executable {
        parts.push("executable".to_string());
    }
    if let Some(target) = &entry.link_target {
        parts.push(format!("-> {target}"));
    }
    if !entry.sha1.is_empty() {
        parts.push(format!("SHA-1 {}", entry.sha1));
    }
    parts.join(" \u{b7} ")
}

/// DLC row tag when queued.
pub fn pending_dlc_label(app_id: u32) -> Option<String> {
    crate::interface::steam_restart::pending_for(app_id).map(|change| {
        let verb = if change.enable { "enable" } else { "disable" };
        format!("  ({verb} pending until next launch)")
    })
}
