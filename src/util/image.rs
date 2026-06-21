use std::sync::{Arc, Mutex};
use std::thread;

use crate::interface::game::Game;
use crate::util::paths::{icon_exists, icon_save};

pub type ImgBuf = image::ImageBuffer<image::Rgba<u8>, Vec<u8>>;

/// Shared slot a background loader writes its `(app_id, image)` result into for
/// the UI thread to pick up.
pub type ImageSlot = Arc<Mutex<Option<(i32, ImgBuf)>>>;

/// Decode a locally-cached image (no network). Sniffs the format from the bytes
/// rather than the file extension, so cached JPEG artwork decodes regardless of
/// the cache file's suffix.
fn load_cached(id: i32) -> Option<ImgBuf> {
    let path = icon_exists(id).ok()?;
    let bytes = std::fs::read(path).ok()?;
    image::load_from_memory(&bytes).ok().map(|d| d.to_rgba8())
}

/// Download + decode artwork on a background thread, caching it for next time and
/// publishing the result into `slot` tagged with `id`.
fn request(id: i32, url: String, slot: ImageSlot) {
    thread::spawn(move || {
        let Ok(payload) = reqwest::blocking::get(&url) else {
            return;
        };
        let Ok(bytes) = payload.bytes() else {
            return;
        };
        let _ = icon_save(id, &bytes);
        if let Ok(data) = image::load_from_memory(&bytes) {
            if let Ok(mut slot) = slot.lock() {
                *slot = Some((id, data.to_rgba8()));
            }
        }
    });
}

/// Update which game's artwork should be shown. Cheap to call every frame: it
/// only does work when the selection changes. A cached image loads inline (fast,
/// no network); otherwise the stale image is cleared and a background download is
/// kicked off, to be adopted later by [`poll`]. `requested` records the id we
/// last acted on so repeated calls for the same selection are no-ops.
pub fn select(
    game: Option<&Game>,
    requested: &mut Option<i32>,
    img: &mut Option<ImgBuf>,
    slot: &ImageSlot,
) {
    let id = game.map(|g| g.id);
    if id == *requested {
        return;
    }
    *requested = id;
    match game {
        Some(g) => match load_cached(g.id) {
            Some(buf) => *img = Some(buf),
            None => {
                *img = None;
                if let Some(url) = &g.icon_url {
                    request(g.id, url.clone(), slot.clone());
                }
            }
        },
        None => *img = None,
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
