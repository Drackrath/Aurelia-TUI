//! End-to-end checks against a real `aurelia` binary. Ignored by default
//! because they require the `aurelia` CLI (>= 0.1.11) on PATH (or via
//! `AURELIA_BIN`) and an authenticated session. Run explicitly with:
//!
//! ```text
//! AURELIA_BIN=/path/to/aurelia cargo test --test aurelia_integration -- --ignored
//! ```

use aurelia_tui::interface::aurelia;
use aurelia_tui::interface::game;

#[test]
#[ignore]
fn health_reports_session() {
    let health = aurelia::health().expect("health check should parse");
    assert!(health.logged_in, "expected an authenticated session");
    assert!(health.account.is_some(), "expected an account name");
}

#[test]
#[ignore]
fn library_loads_and_maps_to_games() {
    // A single fetch, mapped 1:1 (a second `aurelia list` can legitimately
    // return a different count as family-shared/online data resolves).
    let raw = aurelia::fetch_library().expect("list --json should parse");
    assert!(!raw.is_empty(), "expected a non-empty library");

    let games: Vec<_> = raw
        .iter()
        .cloned()
        .map(game::Game::from_library)
        .collect();
    assert_eq!(games.len(), raw.len());

    // The mapped Game should preserve identity and install state.
    let first = &raw[0];
    let mapped = &games[0];
    assert_eq!(mapped.id, first.app_id as i32);
    assert_eq!(mapped.installed, first.is_installed);
    // store_url is carried over as the homepage.
    if let Some(url) = &first.store_url {
        assert_eq!(&mapped.homepage, url);
    }

    // load_library() does its own fetch+map; just confirm it succeeds and is
    // non-empty (its count can differ from `raw` between invocations).
    let loaded = game::load_library().expect("library should map into Games");
    assert!(!loaded.is_empty());
}

#[test]
#[ignore]
fn https_proton_lookup_does_not_abort() {
    // Regression: on Windows the native-TLS backend (schannel 0.1.20) aborted
    // with a UB check during the TLS handshake. With rustls this HTTPS request
    // must complete (returning Some/None) without crashing the process.
    let _ = aurelia_tui::interface::proton_data::ProtonData::get(1888160);
}

#[test]
#[ignore]
fn classic_login_rejects_bad_credentials() {
    use aurelia_tui::interface::aurelia::LoginPhase;
    use std::sync::mpsc::channel;
    use std::sync::{Arc, Mutex};

    let phase = Arc::new(Mutex::new(LoginPhase::Idle));
    let (_tx, rx) = channel();
    let result =
        aurelia::login_classic("not_a_real_user_xyz", "definitely-wrong-password", &phase, rx);

    assert!(result.is_err(), "bad credentials should not log in");
    let final_phase = phase.lock().unwrap().clone();
    match final_phase {
        LoginPhase::Failed(msg) => assert!(!msg.is_empty(), "expected a failure message"),
        other => panic!("expected Failed phase, got {:?}", other),
    }
}

#[test]
#[ignore]
fn friends_search_resolves_a_known_account() {
    // `friends search` is read-only and needs no login: it resolves a SteamID64
    // / profile URL / vanity to a concrete account. Gabe Newell's well-known
    // SteamID64 is a stable fixture.
    let found = aurelia::friends_search("76561197960287930").expect("friends search --json");
    assert_eq!(found.steam_id, 76561197960287930);
    assert!(!found.display_name().is_empty(), "expected a display name");
}

#[test]
#[ignore]
fn info_parses_developers_and_publishers() {
    let raw = aurelia::fetch_library().expect("list");
    let app_id = raw[0].app_id as i32;
    let info = aurelia::fetch_info(app_id).expect("info --json should parse");
    assert_eq!(info.app_id, app_id as u32);
    assert!(!info.name.is_empty());
}

#[test]
#[ignore]
fn market_search_then_price_round_trips() {
    // Both calls are read-only and need no login. Search for a well-known TF2
    // item, then price the first result by its (app_id, market hash name).
    let results = aurelia::market_search("Mann Co").expect("market search --json should parse");
    assert!(!results.is_empty(), "expected at least one market result");

    let first = &results[0];
    assert!(!first.market_hash_name.is_empty(), "result has a hash name");

    let price = aurelia::market_price(first.app_id, &first.market_hash_name)
        .expect("market price --json should parse");
    // The CLI echoes the queried name back; at least one price field should be set.
    assert_eq!(price.market_hash_name, first.market_hash_name);
    assert!(
        price.summary().is_some(),
        "expected some price/volume data for a popular item"
    );
}
