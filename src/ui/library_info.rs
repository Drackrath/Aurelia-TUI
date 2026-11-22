//! Library info rows, tags and labels.

use pretty_bytes::converter::convert;

use tui::text::{Span, Spans, Text};
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

/// Tags, plus size when wide.
pub fn list_suffix(game: &Game, inner_width: usize) -> Option<String> {
    let mut parts: Vec<String> = list_tags(game).into_iter().map(String::from).collect();
    if inner_width >= 48
        && game.installed
        && let Some(bytes) = game.library.installed_size()
    {
        parts.push(convert(bytes as f64));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

fn kv(label: &str, value: String) -> (Row<'static>, u16) {
    (
        Row::new(vec![
            Cell::from(Span::styled(label.to_string(), theme::label())),
            Cell::from(Span::styled(value, theme::value())),
        ]),
        1,
    )
}

fn wrapped(label: &str, text: &str, width: usize, limit: usize) -> Option<(Row<'static>, u16)> {
    let mut lines = super::detail::wrap(text, width.max(1));
    if text.trim().is_empty() || limit == 0 {
        return None;
    }
    if lines.len() > limit {
        lines.truncate(limit);
        if let Some(last) = lines.last_mut() {
            last.push('…');
        }
    }
    let h = lines.len() as u16;
    let text = Text::from(
        lines
            .into_iter()
            .map(|l| Spans::from(Span::styled(l, theme::value())))
            .collect::<Vec<_>>(),
    );
    Some((
        Row::new(vec![
            Cell::from(Span::styled(label.to_string(), theme::label())),
            Cell::from(text),
        ])
        .height(h),
        h,
    ))
}

/// Playtime and library flag rows.
pub fn library_rows(game: &Game) -> Vec<(Row<'static>, u16)> {
    let lib = &game.library;
    let mut rows = vec![kv(
        "Playtime",
        format_playtime(lib.playtime_forever_minutes),
    )];
    if lib.update_queued {
        rows.push(kv("Update", "queued".into()));
    }
    if lib.online_only() {
        rows.push(kv("Online", "required to play".into()));
    }
    rows
}

/// `info --extended` rows; more when expanded.
pub fn extended_rows(game: &Game, width: u16, expand: bool) -> Vec<(Row<'static>, u16)> {
    let Some(ext) = game.extended_info() else {
        return Vec::new();
    };
    let width = width as usize;
    let mut rows = Vec::new();
    if !ext.genres.is_empty() {
        rows.push(kv("Genres", ext.genres.join(", ")));
    }
    if let Some(score) = ext.metacritic {
        rows.push(kv("Metacritic", score.to_string()));
    }
    if !expand {
        return rows;
    }
    if let Some(site) = ext.website.filter(|s| !s.is_empty()) {
        rows.push(kv("Website", site));
    }
    let req = &ext.requirements;
    let all = usize::MAX;
    rows.extend(wrapped("Features", &ext.categories.join(", "), width, all));
    rows.extend(wrapped("Minimum", &req.minimum.join("\n"), width, all));
    rows.extend(wrapped(
        "Recommended",
        &req.recommended.join("\n"),
        width,
        all,
    ));
    rows
}
