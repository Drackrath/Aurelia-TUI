//! Renders themed widgets into tui's off-screen `TestBackend` and asserts the
//! Steam palette is actually painted into the cell buffer (not just that the
//! code compiles).

use aurelia_tui::app::App;
use aurelia_tui::theme;
use tui::backend::TestBackend;
use tui::widgets::Paragraph;
use tui::Terminal;

#[test]
fn panel_paints_steam_palette() {
    let backend = TestBackend::new(24, 6);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            let p = Paragraph::new("hello")
                .block(theme::panel("Title".to_string()))
                .style(theme::base());
            f.render_widget(p, f.size());
        })
        .unwrap();

    let buffer = terminal.backend().buffer();

    // An interior cell is the navy panel background.
    assert_eq!(buffer.get(3, 2).bg, theme::BG, "panel interior should be navy");

    // The title text (row 0) is painted in the accent colour somewhere.
    let title_accent = (1..23).any(|x| buffer.get(x, 0).fg == theme::ACCENT);
    assert!(title_accent, "panel title should use the accent colour");
}

#[test]
fn infobox_is_themed() {
    let backend = TestBackend::new(30, 6);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|f| {
            f.render_widget(App::build_loading(), f.size());
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    assert_eq!(
        buffer.get(2, 2).bg,
        theme::BG,
        "info boxes should sit on the navy panel background"
    );
}
