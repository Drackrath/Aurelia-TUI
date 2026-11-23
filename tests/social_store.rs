//! Persisted friend name cache.

use std::path::PathBuf;
use std::time::Instant;

use aurelia_tui::browse::Browser;
use aurelia_tui::interface::aurelia::FriendJson;
use aurelia_tui::interface::social::UserProfileJson;
use aurelia_tui::social::names::{
    self, NAME_MAX_AGE, NameResolver, NameStore, StoredName, load_store, save_store,
};

const A: u64 = 76561197985686146;
const B: u64 = 76561197989654208;
const NOW: u64 = 1_800_000_000;

fn dir(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn friend(id: u64, name: Option<&str>) -> FriendJson {
    FriendJson {
        steam_id: id,
        persona_name: name.map(Into::into),
        ..Default::default()
    }
}

fn store(entries: &[(u64, &str, u64)]) -> NameStore {
    entries
        .iter()
        .map(|(id, name, at)| {
            (
                id.to_string(),
                StoredName {
                    name: (*name).into(),
                    fetched_at: *at,
                },
            )
        })
        .collect()
}

#[test]
fn round_trip_is_atomic() {
    let path = dir("store-roundtrip").join(names::STORE_FILE);
    let s = store(&[(A, "Magadanch1k", NOW)]);
    save_store(&path, &s).unwrap();
    assert_eq!(load_store(&path), s);
    let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap())
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "temp file renamed away");
}

#[test]
fn corrupt_or_missing_file_is_empty() {
    let d = dir("store-corrupt");
    let path = d.join(names::STORE_FILE);
    assert!(load_store(&path).is_empty());
    std::fs::write(&path, "{not json").unwrap();
    assert!(load_store(&path).is_empty());

    // A later save replaces it.
    let mut r = NameResolver::default();
    r.load_once(&path);
    r.learn_at(
        &UserProfileJson {
            steam_id: A,
            persona_name: Some("Magadanch1k".into()),
            ..Default::default()
        },
        NOW,
    );
    r.save_if_dirty(&path);
    assert_eq!(load_store(&path).len(), 1);
}

#[test]
fn cached_names_show_right_away() {
    let mut b = Browser::new(vec![]);
    b.friends = vec![friend(A, None), friend(B, None)];
    b.social
        .names
        .adopt_store(store(&[(A, "Magadanch1k", NOW)]));
    b.poll_social_at(Instant::now(), false);
    assert_eq!(b.friends[0].display_name(), "Magadanch1k");
    assert_eq!(
        names::missing_note(&b.friends, true)[1],
        "1 left · close Steam for all"
    );
}

#[test]
fn only_stale_cached_names_refresh() {
    let mut r = NameResolver::default();
    r.adopt_store(store(&[
        (A, "Old", NOW - NAME_MAX_AGE),
        (B, "Fresh", NOW - 60),
    ]));
    r.visible_rows.set(10);
    let mut friends = vec![friend(A, None), friend(B, None)];
    r.apply(&mut friends);
    assert_eq!(r.next_lookup_at(&friends, 0, NOW), Some(A));
    r.attempted.insert(A);
    assert_eq!(r.next_lookup_at(&friends, 0, NOW), None, "fresh B skipped");

    // Real Aurelia names never trigger lookups.
    let r = NameResolver::default();
    r.visible_rows.set(10);
    assert_eq!(r.next_lookup_at(&[friend(A, Some("Real"))], 0, NOW), None);
}

#[test]
fn refresh_replaces_stale_name_and_saves_once() {
    let mut r = NameResolver::default();
    r.adopt_store(store(&[(A, "Old", NOW - NAME_MAX_AGE)]));
    let mut friends = vec![friend(A, None)];
    r.apply(&mut friends);
    let p = UserProfileJson {
        steam_id: A,
        persona_name: Some("New".into()),
        ..Default::default()
    };
    r.learn_at(&p, NOW);
    assert!(r.store_dirty);
    assert_eq!(r.store[&A.to_string()].fetched_at, NOW);
    r.apply(&mut friends);
    assert_eq!(friends[0].display_name(), "New");

    // Same name again: nothing to save.
    r.store_dirty = false;
    r.learn_at(&p, NOW + 5);
    assert!(!r.store_dirty);
}

#[test]
fn cache_never_overrides_real_names() {
    let mut r = NameResolver::default();
    r.adopt_store(store(&[(A, "Cached", NOW)]));
    let mut friends = vec![friend(A, Some("Real"))];
    r.apply(&mut friends);
    assert_eq!(friends[0].display_name(), "Real");
}
