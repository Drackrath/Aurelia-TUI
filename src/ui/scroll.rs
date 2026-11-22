//! Scroll window for list overlays.

/// Visible index range keeping `selected` on screen.
pub fn window(selected: usize, len: usize, height: usize) -> (usize, usize) {
    if height == 0 || len == 0 {
        return (0, 0);
    }
    if len <= height {
        return (0, len);
    }
    let selected = selected.min(len - 1);
    // Keep selection inside the window.
    let start = (selected + 1).saturating_sub(height);
    (start, start + height)
}
