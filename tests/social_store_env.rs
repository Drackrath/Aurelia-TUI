//! Name cache through the cache dir.

use std::sync::{Arc, Mutex};

use aurelia_tui::browse::Browser;
use aurelia_tui::interface::aurelia::FriendJson;
use aurelia_tui::interface::social::UserProfileJson;
use aurelia_tui::social::ProfileView;
use aurelia_tui::social::names::{self, load_store};

#[test]
fn poll_loads_and_saves_cache_dir_store() {
    let d = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("store-env");
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let path = d.join(names::STORE_FILE);
    std::fs::write(
        &path,
        r#"{"76561197985686146":{"name":"Magadanch1k","fetched_at":4000000000}}"#,
    )
    .unwrap();
    unsafe {
        std::env::set_var("AURELIA_TUI_DIR", &d);
        std::env::set_var("AURELIA_TUI_CACHE_DIR", &d);
        std::env::set_var("AURELIA_BIN", d.join("missing-backend"));
    }

    let mut b = Browser::new(vec![]);
    b.friends = vec![
        FriendJson {
            steam_id: 76561197985686146,
            ..Default::default()
        },
        FriendJson {
            steam_id: 76561197989654208,
            ..Default::default()
        },
    ];
    b.poll_social();
    assert_eq!(b.friends[0].display_name(), "Magadanch1k");

    // A profile lookup is written back.
    b.social.profile = Some(ProfileView {
        result: Arc::new(Mutex::new(Some(Ok(UserProfileJson {
            steam_id: 76561197989654208,
            persona_name: Some("Second".into()),
            ..Default::default()
        })))),
        ..Default::default()
    });
    b.poll_social();
    let saved = load_store(&path);
    assert_eq!(saved["76561197989654208"].name, "Second");
    assert_eq!(saved["76561197985686146"].name, "Magadanch1k");
}
