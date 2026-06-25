use std::sync::{Arc, Mutex};
use std::thread;

use crate::interface::game::Game;
use crate::util::paths::{icon_exists, icon_save};

pub type ImgBuf = image::ImageBuffer<image::Rgba<u8>, Vec<u8>>;

/// Shared slot a background loader writes its `(app_id, image)` result into for
/// the UI thread to pick up.
pub type ImageSlot = Arc<Mutex<Option<(i32, ImgBuf)>>>;

/// Which artwork to load for a game — they cache and download independently.
#[derive(Clone, Copy)]
pub enum ImageKind {
    /// The wide art used as the detail-pane background (`Game::icon_url`).
    Background,
    /// The portrait box/cover art (`Game::cover_url`).
    Cover,
}

impl ImageKind {
    /// Cache-file suffix so each kind caches under its own name.
    fn suffix(self) -> &'static str {
        match self {
            ImageKind::Background => "",
            ImageKind::Cover => "_cover",
        }
    }

    /// The URL for this kind on `game`.
    fn url(self, game: &Game) -> Option<String> {
        match self {
            ImageKind::Background => game.icon_url.clone(),
            ImageKind::Cover => game.cover_url.clone(),
        }
    }
}

/// Decode a locally-cached image (no network). Sniffs the format from the bytes
/// rather than the file extension, so cached JPEG artwork decodes regardless of
/// the cache file's suffix.
fn load_cached(id: i32, kind: ImageKind) -> Option<ImgBuf> {
    let path = icon_exists(id, kind.suffix()).ok()?;
    let bytes = std::fs::read(path).ok()?;
    image::load_from_memory(&bytes).ok().map(|d| d.to_rgba8())
}

/// Load artwork entirely on a background thread so the UI thread never blocks on
/// disk reads or JPEG decoding (which made scrolling janky). Prefers the cached
/// copy; otherwise downloads, caches and decodes. Publishes the result into
/// `slot` tagged with `id` for [`poll`] to adopt.
fn request(id: i32, kind: ImageKind, url: Option<String>, slot: ImageSlot) {
    thread::spawn(move || {
        let buf = match load_cached(id, kind) {
            Some(buf) => Some(buf),
            None => url.and_then(|url| {
                let bytes = reqwest::blocking::get(&url).ok()?.bytes().ok()?;
                let _ = icon_save(id, kind.suffix(), &bytes);
                image::load_from_memory(&bytes).ok().map(|d| d.to_rgba8())
            }),
        };
        if let Some(buf) = buf {
            if let Ok(mut slot) = slot.lock() {
                *slot = Some((id, buf));
            }
        }
    });
}

/// Update which game's artwork should be shown. Cheap to call every frame: it
/// only does work when the selection changes, and even then never blocks — the
/// image (cached or downloaded) is loaded on a background thread and adopted
/// later by [`poll`]. `requested` records the id we last acted on so repeated
/// calls for the same selection are no-ops.
pub fn select(
    game: Option<&Game>,
    kind: ImageKind,
    requested: &mut Option<i32>,
    img: &mut Option<ImgBuf>,
    slot: &ImageSlot,
) {
    let id = game.map(|g| g.id);
    if id == *requested {
        return;
    }
    *requested = id;
    *img = None;
    if let Some(g) = game {
        request(g.id, kind, kind.url(g), slot.clone());
    }
}

/// Adopt a background-loaded image, but only if it still matches the current
/// selection (a slower download for a game we've since scrolled past is dropped).
pub fn poll(requested: Option<i32>, img: &mut Option<ImgBuf>, slot: &ImageSlot) {
    if let Ok(mut slot) = slot.lock() {
        if let Some((id, buf)) = slot.take() {
            if Some(id) == requested {
                *img = Some(buf);
            }
        }
    }
}
