//! The launch-options overlay (modal popup): the different ways Steam can start
//! the selected game (a normal launch, a level editor, an OS-specific binary,
//! ...), scrollable.

use tui::text::{Span, Spans};
use tui::widgets::Paragraph;

use crate::browse::Browser;
use crate::theme;
use crate::ui::paneled_paragraph;

/// Build the launch-options overlay content from the browse state. Each option
/// renders as its description, the command (executable + arguments), and an OS
/// tag. The list is scrolled by `browser.launch_scroll`.
pub fn launch(browser: &Browser) -> Paragraph<'static> {
    let items = &browser.launch_options;
    let title = format!("Launch options ({})", items.len());

    if items.is_empty() {
        return paneled_paragraph(Span::styled("No launch options.", theme::dim()), title);
    }

    let lines: Vec<Spans<'static>> = items
        .iter()
        .skip(browser.launch_scroll)
        .map(|o| {
            let mut spans = vec![Span::styled(o.display_name(), theme::value())];
            let command = o.command();
            if !command.is_empty() {
                spans.push(Span::styled(format!("  {}", command), theme::dim()));
            }
            spans.push(Span::styled(format!("  [{}]", o.os_tag()), theme::accent()));
            Spans::from(spans)
        })
        .collect();

    paneled_paragraph(lines, title)
}
