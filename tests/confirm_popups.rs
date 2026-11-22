//! Confirm popups fit at both sizes.

use aurelia_tui::ui::confirm;
use tui::Terminal;
use tui::backend::TestBackend;

const LONG: &str = "The Elder Scrolls V: Skyrim Special Edition Anniversary Upgrade Bundle Deluxe";

fn shown(w: u16, h: u16, prompt: String) -> String {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| confirm::draw(f, prompt)).unwrap();
    let buf = t.backend().buffer();
    (0..h)
        .map(|y| {
            (0..w)
                .map(|x| buf.get(x, y).symbol.clone())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn every_prompt_shows_question_and_keys() {
    let prompts = [
        confirm::uninstall_text("Portal 2"),
        confirm::uninstall_text(LONG),
        confirm::remove_friend_text("Rabscuttle"),
        confirm::reconnect_text(),
        confirm::install_text("Half-Life"),
        confirm::install_text(LONG),
    ];
    for (w, h) in [(80, 24), (120, 40)] {
        for prompt in &prompts {
            let out = shown(w, h, prompt.clone());
            assert!(out.contains("[y] confirm"), "{w}x{h}\n{out}");
            assert!(out.contains("[n] cancel"), "{w}x{h}\n{out}");
            for word in prompt.split_whitespace() {
                assert!(out.contains(word), "{w}x{h} lost {word}\n{out}");
            }
        }
    }
}

#[test]
fn popup_sizes_to_text_with_minimum() {
    let frame = tui::layout::Rect::new(0, 0, 80, 24);
    let short = confirm::popup_area(frame, "Remove x?");
    assert_eq!(short.width, 32, "minimum width");
    assert_eq!(short.height, 5);
    let long = confirm::popup_area(frame, &confirm::install_text(LONG));
    assert!(long.width <= 80 && long.height > 5, "{long:?}");
    let wide = confirm::popup_area(
        tui::layout::Rect::new(0, 0, 120, 40),
        &confirm::install_text(LONG),
    );
    assert_eq!(wide.height, 5, "fits one line at 120");
}
