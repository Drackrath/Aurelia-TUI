//! Library info rows, tags and labels.

use pretty_bytes::converter::convert;

use std::sync::atomic::{AtomicU16, Ordering};

use tui::text::Span;
use tui::widgets::{Cell, Row};

use crate::interface::game::Game;
use crate::interface::library_info::format_playtime;
use crate::theme;

/// Installed size; Aurelia's `size_on_disk` wins.
pub fn size_text(game: &Game, fallback: f64) -> String {
    match game.library.installed_size() {
        Some(bytes) => convert(bytes as f64),
        None => convert(fallback),
    }
}

/// Short size, e.g. `13G`, `512M`.
pub fn compact_size(bytes: u64) -> String {
    const GB: f64 = 1e9;
    const MB: f64 = 1e6;
    let b = bytes as f64;
    if b >= 10.0 * GB {
        format!("{:.0}G", b / GB)
    } else if b >= GB {
        format!("{:.1}G", b / GB)
    } else if b >= MB {
        format!("{:.0}M", b / MB)
    } else {
        format!("{:.0}K", (b / 1e3).max(1.0))
    }
}

/// Short list tags, e.g. `queued`, `online`.
pub fn list_tags(game: &Game) -> Vec<&'static str> {
    let mut tags = Vec::new();
    if game.library.update_queued {
        tags.push("queued");
    }
    if game.library.online_only() {
        tags.push("online");
    }
    tags
}

/// List width needed for sizes.
const LIST_SIZE_MIN_WIDTH: usize = 40;

/// Tags, plus size when wide.
pub fn list_suffix(game: &Game, inner_width: usize) -> Option<String> {
    let mut parts: Vec<String> = list_tags(game).into_iter().map(String::from).collect();
    if inner_width >= LIST_SIZE_MIN_WIDTH
        && game.installed
        && let Some(bytes) = game.library.installed_size()
    {
        parts.push(compact_size(bytes));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

/// Playtime and library flag lines.
pub fn library_lines(game: &Game) -> Vec<Line> {
    let lib = &game.library;
    let mut lines = vec![(
        "Playtime".to_string(),
        format_playtime(lib.playtime_forever_minutes),
    )];
    if lib.update_queued {
        lines.push(("Update".into(), "queued".into()));
    }
    if lib.online_only() {
        lines.push(("Online".into(), "required to play".into()));
    }
    lines
}

/// Value width below which labels shorten.
const NARROW_VALUE_WIDTH: u16 = 52;

/// Whether detail labels need short forms.
pub fn is_narrow(value_width: u16) -> bool {
    value_width < NARROW_VALUE_WIDTH
}

/// Long or short label by width.
pub fn label(long: &str, short: &str, narrow: bool) -> String {
    if narrow { short } else { long }.to_string()
}

/// One detail line: label, text.
pub type Line = (String, String);

/// Max scroll offset of last render.
static MAX_SCROLL: AtomicU16 = AtomicU16::new(0);

/// Lines per detail page.
static PAGE: AtomicU16 = AtomicU16::new(1);

/// Expanded-detail page size.
pub fn page() -> u16 {
    PAGE.load(Ordering::Relaxed)
}

/// Largest useful expanded-detail scroll.
pub fn max_scroll() -> u16 {
    MAX_SCROLL.load(Ordering::Relaxed)
}

/// Wrap `text` into labelled lines.
fn push_wrapped(out: &mut Vec<Line>, label: &str, text: &str, width: usize, limit: usize) {
    if text.trim().is_empty() || limit == 0 {
        return;
    }
    let mut lines = super::detail::wrap(text, width.max(1));
    if lines.len() > limit {
        lines.truncate(limit);
        if let Some(last) = lines.last_mut() {
            while last.chars().count() >= width.max(1) {
                last.pop();
            }
            last.push('…');
        }
    }
    for (i, line) in lines.into_iter().enumerate() {
        let label = if i == 0 { label } else { "" };
        out.push((label.to_string(), line));
    }
}

/// Drop repeated entries, keep order.
pub fn dedupe(items: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    items
        .iter()
        .filter(|i| seen.insert(i.to_lowercase()))
        .cloned()
        .collect()
}

/// Extended info and description lines.
pub fn extra_lines(
    game: &Game,
    width: u16,
    expand: bool,
    desc_limit: usize,
    narrow: bool,
) -> Vec<Line> {
    let width = width as usize;
    let mut out = Vec::new();
    if let Some(ext) = game.extended_info() {
        if !ext.genres.is_empty() {
            push_wrapped(&mut out, "Genres", &ext.genres.join(", "), width, 1);
        }
        if let Some(score) = ext.metacritic {
            out.push((label("Metacritic", "Meta", narrow), score.to_string()));
        }
        if expand {
            if let Some(site) = ext.website.as_deref().filter(|s| !s.is_empty()) {
                out.push(("Website".into(), site.to_string()));
            }
            let all = usize::MAX;
            let features = dedupe(&ext.categories).join(", ");
            push_wrapped(&mut out, "Features", &features, width, all);
            let req = &ext.requirements;
            push_wrapped(&mut out, "Minimum", &req.minimum.join("\n"), width, all);
            let rec = req.recommended.join("\n");
            let rec_label = label("Recommended", "Recomm.", narrow);
            push_wrapped(&mut out, &rec_label, &rec, width, all);
        }
    }
    let limit = if expand { usize::MAX } else { desc_limit };
    let desc_label = label("Description", "About", narrow);
    push_wrapped(&mut out, &desc_label, &game.get_description(), width, limit);
    out
}

/// Visible lines with scroll markers.
pub fn visible_lines(
    lines: Vec<Line>,
    budget: usize,
    scroll: u16,
    expand: bool,
    record: bool,
) -> Vec<Line> {
    if budget == 0 {
        return Vec::new();
    }
    if !expand {
        return truncated(lines, budget);
    }
    let max = lines.len().saturating_sub(budget);
    // Only real renders set limits.
    if record {
        MAX_SCROLL.store(max.min(u16::MAX as usize) as u16, Ordering::Relaxed);
        PAGE.store(budget.saturating_sub(1).max(1) as u16, Ordering::Relaxed);
    }
    let start = (scroll as usize).min(max);
    let end = (start + budget).min(lines.len());
    let mut shown: Vec<Line> = lines[start..end].to_vec();
    if start > 0
        && let Some(first) = shown.first_mut()
        && first.0.is_empty()
    {
        first.0 = "  ▲".into();
    }
    // Always flag hidden lines below.
    if end < lines.len()
        && let Some(last) = shown.last_mut()
    {
        *last = ("  ▼".into(), "more · PgDn".into());
    }
    shown
}

/// First `budget` lines, ellipsis if cut.
fn truncated(mut lines: Vec<Line>, budget: usize) -> Vec<Line> {
    if lines.len() > budget {
        lines.truncate(budget);
        if let Some(last) = lines.last_mut() {
            last.1.push('…');
        }
    }
    lines
}

/// Detail rows for visible lines.
pub fn line_rows(lines: Vec<Line>) -> Vec<Row<'static>> {
    lines
        .into_iter()
        .map(|(label, text)| {
            let style = if label.trim_start().starts_with(['▲', '▼']) {
                theme::dim()
            } else {
                theme::label()
            };
            Row::new(vec![
                Cell::from(Span::styled(label, style)),
                Cell::from(Span::styled(text, theme::value())),
            ])
        })
        .collect()
}

/// Detail panel title with key hints.
pub fn detail_title(expand: bool, has_game: bool) -> &'static str {
    match (has_game, expand) {
        (false, _) => "Detail",
        (true, false) => "Detail · [i] more",
        (true, true) => "Detail · [i] less · PgUp/PgDn scroll",
    }
}
