//! The versions & pinning overlay (modal popup): the selected game's per-depot
//! current manifests, its pin state, and a prompt to downgrade the highlighted
//! depot to a specific (usually older) manifest id.

use pretty_bytes::converter::convert;

use tui::style::{Modifier, Style};
use tui::text::{Span, Spans};
use tui::widgets::Paragraph;

use crate::browse::{Browser, VersionRow};
use crate::theme;
use crate::ui::{paneled_paragraph, selection_marker};

/// Footer key hints, wrapped to width.
const HINT: &str =
    "[j/k] move  [g/G] ends  [p] pin  [u] unpin  [d] downgrade  [s] SteamDB  [Esc] close";

/// Unsized variant; shows every row.
pub fn versions(browser: &Browser) -> Paragraph<'static> {
    versions_sized(browser, 80, u16::MAX)
}

/// Inner width needing compact rows.
const NARROW: usize = 70;

/// Narrow: last 8 manifest digits.
pub fn short_manifest(id: u64, narrow: bool) -> String {
    let text = id.to_string();
    let n = text.chars().count();
    if narrow && n > 9 {
        format!("…{}", &text[n - 8..])
    } else {
        text
    }
}

fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn row_line(r: &VersionRow, selected: bool, narrow: bool) -> Spans<'static> {
    let style = if selected {
        theme::selection(theme::ACCENT)
    } else {
        theme::value()
    };
    let name = match &r.depot_name {
        Some(n) if !narrow => format!("{n} ({})", r.depot_id),
        _ => r.depot_id.to_string(),
    };
    let mut spans = vec![
        Span::styled(selection_marker(selected).to_string(), style),
        Span::styled(name, style),
    ];
    if let Some(branch) = &r.branch {
        let branch = if narrow {
            clip(branch, 12)
        } else {
            branch.clone()
        };
        spans.push(Span::styled(format!(" [{branch}]"), theme::accent()));
    }
    match r.manifest_id {
        Some(mid) => spans.push(Span::styled(
            format!("  {}", short_manifest(mid, narrow)),
            theme::dim(),
        )),
        None => {
            if let Some(mid) = r.installed {
                let label = if narrow { "inst" } else { "installed" };
                spans.push(Span::styled(
                    format!("  {label} {}", short_manifest(mid, narrow)),
                    theme::dim(),
                ));
            }
            spans.push(Span::styled(" (unlisted)".to_string(), theme::dim()));
        }
    }
    if let Some(size) = r.size {
        spans.push(Span::styled(
            format!("  {}", convert(size as f64)),
            theme::dim(),
        ));
    }
    if r.is_installed() {
        let text = if narrow { " · inst" } else { " · installed" };
        spans.push(Span::styled(
            text.to_string(),
            theme::value().fg(theme::ONLINE),
        ));
    }
    if let Some(pin) = r.pinned {
        let note = if r.manifest_id == Some(pin) {
            " · pinned".to_string()
        } else if r.manifest_id.is_none() {
            format!(" · pinned {}", short_manifest(pin, narrow))
        } else {
            " · pinned elsewhere".to_string()
        };
        spans.push(Span::styled(note, theme::value().fg(theme::WARN)));
    }
    Spans::from(spans)
}

/// Wrap footer text into styled lines.
fn footer_lines(text: &str, width: usize, style: Style) -> Vec<Spans<'static>> {
    super::detail::wrap(text, width)
        .into_iter()
        .map(|l| Spans::from(Span::styled(l, style)))
        .collect()
}

/// Sized overlay; fixed header, footer.
pub fn versions_sized(browser: &Browser, width: u16, height: u16) -> Paragraph<'static> {
    let inner_w = width.saturating_sub(2).max(1) as usize;
    let mut header: Vec<Spans<'static>> = Vec::new();
    let pin_span = if browser.is_pinned() {
        Span::styled(
            "PINNED (updates held)".to_string(),
            theme::value().fg(theme::WARN).add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled("not pinned".to_string(), theme::dim())
    };
    header.push(Spans::from(vec![
        Span::styled("Version pin: ".to_string(), theme::label()),
        pin_span,
    ]));
    if let Some(path) = browser
        .versions_available
        .as_ref()
        .and_then(|a| a.install_path.clone())
    {
        header.push(Spans::from(vec![
            Span::styled("Installed at: ".to_string(), theme::label()),
            Span::styled(path, theme::dim()),
        ]));
    }
    header.push(Spans::from(Span::styled(
        "depot [branch]  manifest  size · state".to_string(),
        theme::dim(),
    )));

    // Footer: prompt or status, then hints.
    let mut footer: Vec<Spans<'static>> = vec![Spans::from("")];
    if let Some(confirm) = browser.versions_confirm {
        footer.extend(footer_lines(&confirm.prompt(), inner_w, theme::accent()));
        footer.extend(footer_lines(
            "[y] confirm · any other key cancels",
            inner_w,
            theme::key(),
        ));
    } else if let Some(input) = &browser.versions_input {
        footer.push(Spans::from(vec![
            Span::styled("Downgrade to manifest id: ".to_string(), theme::label()),
            Span::styled(input.clone(), theme::accent()),
            Span::styled("_".to_string(), theme::key()),
        ]));
        footer.extend(footer_lines(
            "[Enter] continue · [Esc] cancel · older ids: SteamDB",
            inner_w,
            theme::dim(),
        ));
    } else {
        let status = if !browser.versions_status.is_empty() {
            Some(browser.versions_status.clone())
        } else {
            browser.downgrade_busy_text()
        };
        if let Some(status) = status {
            footer.extend(footer_lines(&status, inner_w, theme::value()));
        }
        footer.extend(footer_lines(HINT, inner_w, theme::dim()));
    }

    let rows = browser.versions_rows();
    let inner_h = height.saturating_sub(2) as usize;
    let list_h = inner_h.saturating_sub(header.len() + footer.len()).max(1);
    let mut lines = header;
    if rows.is_empty() {
        lines.push(Spans::from(Span::styled(
            "No depot manifests (the game may not be installed or owned).".to_string(),
            theme::dim(),
        )));
    } else {
        let (start, end) = super::scroll::window(browser.versions_index, rows.len(), list_h);
        for (i, r) in rows.iter().enumerate().take(end).skip(start) {
            lines.push(row_line(r, i == browser.versions_index, inner_w < NARROW));
        }
    }
    // Pin footer to bottom.
    let used = lines.len() + footer.len();
    if height != u16::MAX && used < inner_h {
        lines.extend((used..inner_h).map(|_| Spans::from("")));
    }
    lines.extend(footer);

    let title = if rows.is_empty() {
        "Versions & pinning".to_string()
    } else {
        format!(
            "Versions & pinning ({}/{})",
            browser.versions_index.min(rows.len() - 1) + 1,
            rows.len()
        )
    };
    paneled_paragraph(lines, title)
}
