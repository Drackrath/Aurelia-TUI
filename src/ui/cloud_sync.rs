//! Cloud conflict prompt and sync badges.

use tui::style::Style;
use tui::text::{Span, Spans};
use tui::widgets::{Paragraph, Wrap};

use crate::interface::aurelia::CloudDirection;
use crate::interface::cloud_status::{self, CloudBadge, CloudRecord, ago};
use crate::interface::cloud_sync::ConflictPrompt;
use crate::theme;
use crate::ui::paneled_paragraph;

/// Most conflicting files listed.
const MAX_FILES: usize = 8;

/// The "keep cloud / local" prompt.
pub fn conflict(prompt: &ConflictPrompt, game_name: Option<&str>, now: u64) -> Paragraph<'static> {
    let game = game_name
        .map(str::to_string)
        .unwrap_or_else(|| format!("app {}", prompt.app_id));
    let pass = match prompt.direction {
        CloudDirection::Both => "sync",
        CloudDirection::Down => "download",
        CloudDirection::Up => "upload",
    };
    let mut lines = vec![
        Spans::from(Span::styled(
            format!(
                "The Cloud and local saves for {game} have diverged ({pass}). Both changed since they last matched; keep which copy?"
            ),
            Style::default().fg(theme::TEXT),
        )),
        Spans::from(""),
    ];
    for c in prompt.conflicts.iter().take(MAX_FILES) {
        lines.push(Spans::from(Span::styled(
            format!("• {}", c.filename),
            theme::value(),
        )));
        lines.push(Spans::from(Span::styled(
            format!(
                "    cloud {} B, {}   local {} B, {}",
                c.cloud_size,
                ago(now, c.cloud_timestamp),
                c.local_size,
                ago(now, c.local_timestamp)
            ),
            theme::dim(),
        )));
    }
    if prompt.conflicts.len() > MAX_FILES {
        lines.push(Spans::from(Span::styled(
            format!("  … and {} more", prompt.conflicts.len() - MAX_FILES),
            theme::dim(),
        )));
    }
    lines.push(Spans::from(""));
    lines.push(Spans::from(vec![
        Span::styled("[c]", theme::key()),
        Span::styled(" keep cloud   ", theme::dim()),
        Span::styled("[l]", theme::key()),
        Span::styled(" keep local   ", theme::dim()),
        Span::styled("[Esc]", theme::key()),
        Span::styled(" leave both", theme::dim()),
    ]));
    paneled_paragraph(lines, "Steam Cloud save conflict".to_string()).wrap(Wrap { trim: false })
}

/// Short list marker per state.
pub fn marker(record: &CloudRecord) -> Span<'static> {
    match record.state {
        CloudBadge::Syncing => Span::styled(" ☁…", theme::accent()),
        CloudBadge::Ok => Span::styled(" ☁", theme::dim()),
        CloudBadge::Conflict => Span::styled(" ☁!", Style::default().fg(theme::WARN)),
        CloudBadge::Error => Span::styled(" ☁✗", theme::item_failed()),
    }
}

/// List marker, if recorded.
pub fn list_marker(app_id: i32) -> Option<Span<'static>> {
    cloud_status::get(app_id).map(|r| marker(&r))
}

/// Detail-pane text for a record.
pub fn describe(record: &CloudRecord, now: u64) -> String {
    let last = record
        .last_success
        .map(|t| format!("last ok {}", ago(now, t)))
        .unwrap_or_else(|| "never synced ok".to_string());
    let first_line = record
        .message
        .as_deref()
        .and_then(|m| m.lines().next())
        .unwrap_or("");
    match record.state {
        CloudBadge::Syncing => format!("syncing… ({last})"),
        CloudBadge::Ok => format!("synced {}", ago(now, record.updated)),
        CloudBadge::Conflict => format!(
            "{} conflict(s), open [C] to resolve ({last})",
            record.conflicts
        ),
        CloudBadge::Error if first_line.is_empty() => format!("error ({last})"),
        CloudBadge::Error => format!("error: {first_line} ({last})"),
    }
}

/// Detail-pane text for a game.
pub fn detail_value(app_id: i32) -> Option<String> {
    cloud_status::get(app_id).map(|r| describe(&r, cloud_status::now()))
}
