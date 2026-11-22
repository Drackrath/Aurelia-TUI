//! Invite and profile overlays.

use tui::style::{Modifier, Style};
use tui::text::{Span, Spans, Text};
use tui::widgets::{Paragraph, Wrap};

use crate::interface::social::UserProfileJson;
use crate::social::{InviteDraft, InviteField, ProfileView};
use crate::theme;

fn caret() -> Span<'static> {
    Span::styled(
        "_".to_string(),
        Style::default()
            .fg(theme::ACCENT_BRIGHT)
            .add_modifier(Modifier::BOLD),
    )
}

fn field_line(label: &str, value: &str, focused: bool) -> Spans<'static> {
    let mut spans = vec![
        Span::styled(crate::ui::selection_marker(focused), theme::accent()),
        Span::styled(format!("{label:<9}"), theme::label()),
        Span::styled(value.to_string(), theme::value()),
    ];
    if focused {
        spans.push(caret());
    }
    Spans::from(spans)
}

/// The "Invite to game" overlay.
pub fn invite_overlay(draft: &InviteDraft) -> Paragraph<'static> {
    let app_value = match &draft.game_name {
        Some(name) if !draft.app_input.is_empty() => format!("{} ({name})", draft.app_input),
        _ => draft.app_input.clone(),
    };
    let mut lines = vec![
        Spans::from({
            let (name, id) = draft.header();
            let mut spans = vec![
                Span::styled("Invite ", theme::label()),
                Span::styled(
                    name,
                    Style::default()
                        .fg(theme::ACCENT)
                        .add_modifier(Modifier::BOLD),
                ),
            ];
            if let Some(id) = id {
                spans.push(Span::styled(format!("  ({id})"), theme::dim()));
            }
            spans
        }),
        Spans::from(""),
        field_line("App id", &app_value, draft.field == InviteField::App),
        field_line(
            "Connect",
            &draft.connect,
            draft.field == InviteField::Connect,
        ),
        Spans::from(""),
        Spans::from(Span::styled(
            "The game must already run on your account (Steam client or play --steam).".to_string(),
            theme::dim(),
        )),
    ];
    let status = draft.status_text();
    if !status.is_empty() {
        lines.push(Spans::from(""));
        lines.push(Spans::from(Span::styled(
            status,
            Style::default().fg(theme::WARN),
        )));
    }
    Paragraph::new(Text::from(lines))
        .block(theme::panel(
            "Invite · [Enter] send [Tab] field [Esc]".to_string(),
        ))
        .style(theme::base())
        .wrap(Wrap { trim: false })
}

/// Epoch days to civil date.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// Unix seconds as `YYYY-MM-DD`.
pub fn unix_to_ymd(secs: u64) -> String {
    let (y, m, d) = civil_from_days((secs / 86_400) as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Drop HTML tags and common entities.
pub fn strip_html(input: &str) -> String {
    let with_breaks = input
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n");
    let mut out = String::with_capacity(with_breaks.len());
    let mut in_tag = false;
    for c in with_breaks.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .trim()
        .to_string()
}

fn kv(label: &str, value: String) -> Spans<'static> {
    Spans::from(vec![
        Span::styled(format!("{label:<10}"), theme::label()),
        Span::styled(value, theme::value()),
    ])
}

/// Profile rows like `aurelia user`.
pub fn profile_lines(p: &UserProfileJson) -> Vec<Spans<'static>> {
    let mut lines = vec![
        Spans::from(vec![
            Span::styled(
                p.display_name(),
                Style::default()
                    .fg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("  ({})", p.steam_id), theme::dim()),
        ]),
        Spans::from(""),
        kv("Status", p.status_label().to_string()),
    ];
    if let Some(game) = p.playing() {
        lines.push(kv("Playing", game));
    }
    if let Some(url) = p.profile_url.clone().filter(|u| !u.is_empty()) {
        lines.push(kv("Profile", url));
    }
    if let Some(url) = p.avatar_url.clone() {
        lines.push(kv("Avatar", url));
    }
    let limited = if p.is_limited {
        ", limited account"
    } else {
        ""
    };
    lines.push(kv("Privacy", format!("{}{limited}", p.visibility_label())));
    if let Some(name) = p.real_name.clone() {
        lines.push(kv("Name", name));
    }
    if let Some(place) = p.location.clone() {
        lines.push(kv("Location", place));
    }
    if let Some(t) = p.created {
        lines.push(kv("Created", unix_to_ymd(t)));
    }
    if let Some(t) = p.last_logoff {
        lines.push(kv("Last seen", unix_to_ymd(t)));
    }
    if let Some(h) = p.headline.clone() {
        lines.push(kv("Headline", h));
    }
    if let Some(summary) = p.summary.as_deref() {
        lines.push(Spans::from(""));
        for line in strip_html(summary).lines() {
            lines.push(Spans::from(Span::styled(line.to_string(), theme::value())));
        }
    }
    if let Some(t) = p.ban_expires {
        lines.push(Spans::from(""));
        lines.push(Spans::from(Span::styled(
            format!("Community ban until {}", unix_to_ymd(t)),
            Style::default().fg(theme::WARN),
        )));
    }
    lines
}

/// The read-only profile overlay.
pub fn profile_overlay(view: &ProfileView) -> Paragraph<'static> {
    let lines = match view.finished() {
        None => vec![
            Spans::from(Span::styled(
                format!(
                    "Loading profile of {}… {}s",
                    view.label,
                    view.elapsed_secs(std::time::Instant::now())
                ),
                theme::dim(),
            )),
            Spans::from(Span::styled(
                "Steam may take up to the persona wait.".to_string(),
                theme::dim(),
            )),
        ],
        Some(Err(error)) => vec![Spans::from(Span::styled(
            format!("Profile unavailable: {error}"),
            Style::default().fg(theme::WARN),
        ))],
        Some(Ok(profile)) => profile_lines(&profile),
    };
    Paragraph::new(Text::from(lines))
        .block(theme::panel(
            "Profile · [j/k] scroll [Esc] close".to_string(),
        ))
        .style(theme::base())
        .wrap(Wrap { trim: false })
        .scroll((view.scroll, 0))
}

/// Friends tab footer key hints.
pub const FRIEND_HINTS: [(&str, &str); 7] = [
    ("[Enter]", " chat  "),
    ("[t]", " window  "),
    ("[i]", " invite  "),
    ("[p]", " profile  "),
    ("[m]", " me  "),
    ("[a]", " add  "),
    ("[x]", " remove"),
];
