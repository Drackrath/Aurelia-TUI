//! The chat conversation panel: an always-visible / dedicated chat window that
//! renders the message history with a friend plus an input line for composing
//! a new message.
//!
//! Layout & behaviour:
//! - Messages are **bottom-anchored**: the newest message sits on the last row
//!   of the message region (just above the composer separator). When the
//!   history is shorter than the available height, blank rows pad the *top* so
//!   the conversation hugs the bottom. When it overflows, the oldest lines are
//!   dropped so only the most recent visible lines remain.
//! - Messages are aligned **per-message**: the logged-in user's own messages
//!   are RIGHT-aligned, the partner's messages are LEFT-aligned. tui's
//!   `Paragraph` only supports a single global alignment, so we keep that
//!   `Alignment::Left` and align every visual line manually by left-padding
//!   own-message lines with spaces.
//! - Each message body is word-wrapped to the content width (accounting for the
//!   speaker label on the first line), and the bottom two content rows are
//!   reserved for the composer (a separator line and the input line).

use tui::layout::Alignment;
use tui::style::{Modifier, Style};
use tui::text::{Span, Spans, Text};
use tui::widgets::Paragraph;

use crate::browse::Browser;
use crate::theme;

/// One fully-rendered visual line of the message region: the styled spans plus
/// whether it belongs to an own (right-aligned) message. The display width is
/// the number of columns the spans occupy, used for padding/wrapping decisions.
struct RenderedLine {
    spans: Vec<Span<'static>>,
    width: usize,
    from_self: bool,
}

/// Greedily word-wrap `body` into lines whose display width fits `width`,
/// where the FIRST line may only use `first_avail` columns (the rest of the
/// first row being taken by the speaker label). Words longer than the available
/// width are hard-split. Returns at least one (possibly empty) line.
fn wrap_body(body: &str, first_avail: usize, width: usize) -> Vec<String> {
    // Avoid pathological zero-width situations (e.g. an extremely narrow panel)
    // by always allowing at least one column of progress.
    let width = width.max(1);
    let first_avail = first_avail.max(1);

    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    // The width budget for the line currently being built.
    let mut avail = first_avail;

    // Push `current` as a finished line and reset for a continuation line.
    macro_rules! flush {
        () => {{
            lines.push(std::mem::take(&mut current));
            avail = width;
        }};
    }

    for word in body.split_whitespace() {
        // Hard-split words that can never fit on a single line.
        let mut word = word;
        while word.chars().count() > avail {
            // If the current line already has content, start fresh first so we
            // don't split a word across a partially-filled line awkwardly.
            if !current.is_empty() {
                flush!();
                continue;
            }
            // Take exactly `avail` chars onto this line, then continue.
            let take = avail;
            let head: String = word.chars().take(take).collect();
            let tail_start = head.len();
            lines.push(head);
            avail = width;
            word = &word[tail_start..];
        }

        let word_len = word.chars().count();
        let sep = if current.is_empty() { 0 } else { 1 };
        if word_len + sep > avail {
            // Doesn't fit on the current line — wrap to a new one.
            flush!();
        }
        if !current.is_empty() {
            current.push(' ');
            avail = avail.saturating_sub(1);
        }
        current.push_str(word);
        avail = avail.saturating_sub(word_len);
    }

    // Always emit the trailing (or only) line, even if empty, so an empty
    // message body still occupies one row.
    lines.push(current);
    lines
}

/// Render a single chat message into one or more [`RenderedLine`]s.
fn render_message(
    body: &str,
    from_self: bool,
    partner: &str,
    content_w: usize,
) -> Vec<RenderedLine> {
    // Speaker label and its styles.
    let (label, label_style) = if from_self {
        ("me: ".to_string(), theme::dim())
    } else {
        (format!("{partner}: "), theme::accent())
    };
    let label_w = label.chars().count();

    // The first line shares its row with the label; later lines get the full
    // width. Guard against the label being wider than the panel.
    let first_avail = content_w.saturating_sub(label_w);
    let wrapped = wrap_body(body, first_avail, content_w);

    let mut out: Vec<RenderedLine> = Vec::new();
    for (i, text) in wrapped.into_iter().enumerate() {
        let body_w = text.chars().count();
        let mut spans: Vec<Span<'static>> = Vec::new();
        let mut width = 0usize;

        if i == 0 {
            // First visual line carries the speaker label.
            spans.push(Span::styled(label.clone(), label_style));
            width += label_w;
        }
        spans.push(Span::styled(text, theme::value()));
        width += body_w;

        out.push(RenderedLine {
            spans,
            width,
            from_self,
        });
    }
    out
}

/// Build the chat panel from the current browser state.
///
/// `width`/`height` are the FULL render-area dimensions including the panel
/// border; the usable content area is `(width - 2) x (height - 2)`.
pub fn chat(browser: &Browser, width: u16, height: u16) -> Paragraph<'static> {
    let title = format!(
        "Chat — {} ([Esc] close · [Enter] send)",
        browser.chat_partner
    );
    let block = theme::panel(title);

    let content_w = width.saturating_sub(2) as usize;
    let content_h = height.saturating_sub(2);

    // If there isn't even room for the composer's two rows, render whatever we
    // can without panicking — an empty paragraph inside the block.
    if content_h < 2 {
        return Paragraph::new(Text::from(Vec::<Spans<'static>>::new()))
            .block(block)
            .style(theme::base())
            .alignment(Alignment::Left);
    }

    // The message region is everything above the 2 reserved composer rows.
    let msg_h = content_h.saturating_sub(2) as usize;

    // --- Render every message into flat visual lines (chronological order) ---
    let mut rendered: Vec<RenderedLine> = Vec::new();
    if browser.chat_messages.is_empty() {
        // Empty state: a single dim hint line, still bottom-anchored.
        let text = "No messages yet. Say hi!".to_string();
        let width = text.chars().count();
        rendered.push(RenderedLine {
            spans: vec![Span::styled(text, theme::dim())],
            width,
            from_self: false,
        });
    } else {
        for m in &browser.chat_messages {
            rendered.extend(render_message(
                &m.message,
                m.from_self,
                &browser.chat_partner,
                content_w,
            ));
        }
    }

    // --- Overflow: keep only the most recent lines that fit in `msg_h` ---
    if rendered.len() > msg_h {
        let drop = rendered.len() - msg_h;
        rendered.drain(0..drop);
    }

    // --- Convert each rendered line into a finished `Spans`, applying the
    //     per-message alignment via left-padding for own messages. ---
    let mut msg_lines: Vec<Spans<'static>> = Vec::with_capacity(msg_h);
    for line in rendered {
        if line.from_self {
            // Right-align: left-pad with spaces so the right edge hits content_w.
            let pad = content_w.saturating_sub(line.width);
            let mut spans: Vec<Span<'static>> = Vec::with_capacity(line.spans.len() + 1);
            if pad > 0 {
                spans.push(Span::raw(" ".repeat(pad)));
            }
            spans.extend(line.spans);
            msg_lines.push(Spans::from(spans));
        } else {
            // Left-align: no leading pad.
            msg_lines.push(Spans::from(line.spans));
        }
    }

    // --- Top-pad with blank rows so the block hugs the bottom ---
    let mut lines: Vec<Spans<'static>> = Vec::with_capacity(msg_h + 2);
    let pad_rows = msg_h.saturating_sub(msg_lines.len());
    for _ in 0..pad_rows {
        lines.push(Spans::from(Vec::<Span<'static>>::new()));
    }
    lines.extend(msg_lines);

    // --- Composer: separator + input line (the reserved bottom 2 rows) ---
    lines.push(Spans::from(Span::styled(
        "─".repeat(content_w),
        Style::default().fg(theme::BORDER),
    )));
    lines.push(Spans::from(vec![
        Span::styled("> ".to_string(), theme::accent()),
        Span::styled(browser.chat_input.clone(), theme::value()),
        Span::styled(
            "_".to_string(),
            Style::default()
                .fg(theme::ACCENT_BRIGHT)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    // `lines` is now exactly `msg_h + 2` rows tall, for a deterministic layout.
    Paragraph::new(Text::from(lines))
        .block(block)
        .style(theme::base())
        .alignment(Alignment::Left)
}

#[cfg(test)]
mod tests {
    use super::wrap_body;

    /// A single word of multibyte (multi-byte-per-char) glyphs, longer than the
    /// line width and containing no spaces, forces the hard-split path — the
    /// `&word[tail_start..]` byte slice at the heart of `wrap_body`. `tail_start`
    /// must land on a char boundary, or this panics. Locks in that invariant.
    #[test]
    fn wrap_body_hard_splits_multibyte_word_on_char_boundaries() {
        let word = "日".repeat(10); // 10 chars, 30 bytes (3 bytes each)
        let width = 4;
        let lines = wrap_body(&word, width, width);

        // Every produced line fits the width budget measured in CHARS, not bytes.
        for line in &lines {
            assert!(
                line.chars().count() <= width,
                "line {line:?} exceeds {width} columns"
            );
        }
        // The split is lossless: a single word (no spaces) round-trips exactly,
        // which can only hold if every slice fell on a char boundary.
        assert_eq!(lines.concat(), word, "multibyte word must round-trip");
    }

    /// Mixed ASCII + a too-long emoji run exercises the word-wrap and hard-split
    /// paths together; emoji are 4-byte chars, a classic boundary tripwire.
    #[test]
    fn wrap_body_mixed_ascii_and_emoji_does_not_panic() {
        let body = "hi 🎮🎮🎮🎮🎮 ok";
        let lines = wrap_body(body, 3, 3);

        for line in &lines {
            assert!(line.chars().count() <= 3, "line {line:?} exceeds 3 columns");
        }
        // All non-space glyphs survive the wrap (none dropped or corrupted).
        let original: String = body.chars().filter(|c| !c.is_whitespace()).collect();
        let wrapped: String = lines
            .concat()
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        assert_eq!(wrapped, original, "no glyph lost or corrupted across wrap");
    }
}
