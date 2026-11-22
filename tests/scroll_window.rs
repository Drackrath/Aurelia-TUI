use aurelia_tui::ui::scroll::window;

#[test]
fn short_list_shows_everything() {
    assert_eq!(window(2, 5, 10), (0, 5));
}

#[test]
fn selection_stays_visible() {
    assert_eq!(window(0, 30, 10), (0, 10));
    assert_eq!(window(9, 30, 10), (0, 10));
    assert_eq!(window(10, 30, 10), (1, 11));
    assert_eq!(window(29, 30, 10), (20, 30));
}

#[test]
fn degenerate_sizes() {
    assert_eq!(window(0, 0, 10), (0, 0));
    assert_eq!(window(3, 10, 0), (0, 0));
    assert_eq!(window(99, 10, 4), (6, 10));
}
