//! Library fields, extended info, SteamDB links.

use aurelia_tui::browse::{Action, Browser, Sort};
use aurelia_tui::interface::aurelia::{InfoJson, LibraryGameJson};
use aurelia_tui::interface::game::Game;
use aurelia_tui::interface::library_info::{self as info, LibraryInfoJson};
use aurelia_tui::ui;
use tui::{Terminal, backend::TestBackend};

fn isolate_config() {
    let dir = std::env::temp_dir().join(format!("aurelia-library-info-{}", std::process::id()));
    // SAFETY: set before any config read.
    unsafe {
        std::env::set_var("AURELIA_TUI_DIR", &dir);
        std::env::set_var("AURELIA_TUI_CACHE_DIR", &dir);
        std::env::set_var("AURELIA_BIN", dir.join("missing-backend"));
    }
}

fn library() -> Vec<LibraryGameJson> {
    serde_json::from_str(include_str!("fixtures/library_info/list.json")).unwrap()
}

fn games() -> Vec<Game> {
    library().into_iter().map(Game::from_library).collect()
}

#[test]
fn list_json_fields_parse() {
    let entries = library();
    assert_eq!(
        entries[0].library,
        LibraryInfoJson {
            playtime_forever_minutes: Some(754),
            update_queued: true,
            online_required: Some(false),
            size_on_disk: Some(13_421_772_800),
        }
    );
    assert!(entries[1].library.online_only());
    assert_eq!(entries[1].library.installed_size(), None);
    assert_eq!(entries[2].library.playtime_forever_minutes, None);
    assert_eq!(entries[2].library.online_required, None);
    // Existing fields still parse.
    assert_eq!(entries[0].platform.as_deref(), Some("linux"));
}

#[test]
fn older_list_json_defaults() {
    let entry: LibraryGameJson =
        serde_json::from_str(r#"{"app_id": 10, "name": "Old", "is_installed": false}"#).unwrap();
    assert_eq!(entry.library, LibraryInfoJson::default());
}

#[test]
fn game_cache_keeps_library_fields() {
    let game = games().remove(0);
    let restored: Game = serde_json::from_str(&serde_json::to_string(&game).unwrap()).unwrap();
    assert_eq!(restored.library, game.library);
    // Pre-update cache entries still load.
    let mut legacy = serde_json::to_value(&game).unwrap();
    legacy.as_object_mut().unwrap().remove("library");
    let legacy: Game = serde_json::from_value(legacy).unwrap();
    assert_eq!(legacy.library, LibraryInfoJson::default());
}

#[test]
fn playtime_formats() {
    assert_eq!(info::format_playtime(None), "—");
    assert_eq!(info::format_playtime(Some(0)), "never played");
    assert_eq!(info::format_playtime(Some(45)), "45m");
    assert_eq!(info::format_playtime(Some(754)), "12h 34m");
}

#[test]
fn playtime_sort_orders_most_played_first() {
    isolate_config();
    let mut browser = Browser::new(games());
    browser.sort = Sort::Installed;
    browser.cycle_sort();
    assert!(browser.sort == Sort::Playtime);
    assert_eq!(browser.sort.label(), "playtime");
    let ids: Vec<i32> = browser.visible().iter().map(|g| g.id).collect();
    assert_eq!(ids, [730, 620, 400], "unknown playtime last");
    browser.cycle_sort();
    assert!(browser.sort == Sort::Name);
}

#[test]
fn extended_info_parses() {
    let parsed: InfoJson =
        serde_json::from_str(include_str!("fixtures/library_info/info_extended.json")).unwrap();
    assert_eq!(parsed.developers, ["Valve"]);
    let ext = parsed.extended.expect("extended block");
    assert_eq!(ext.genres, ["Action", "Adventure"]);
    assert_eq!(ext.metacritic, Some(95));
    assert_eq!(
        ext.website.as_deref(),
        Some("http://www.thinkwithportals.com/")
    );
    assert_eq!(ext.requirements.minimum.len(), 3);
    assert!(ext.requirements.recommended.is_empty());
    // Plain `info` has no extended block.
    let plain: InfoJson = serde_json::from_str(r#"{"app_id": 620, "name": "Portal 2"}"#).unwrap();
    assert!(plain.extended.is_none());
}

#[test]
fn info_args_add_extended_and_lang() {
    assert_eq!(
        info::info_args("620", Some("german"), true),
        ["info", "620", "--extended", "-l", "german"]
    );
    assert_eq!(info::info_args("620", None, false), ["info", "620"]);
    assert_eq!(
        info::info_args("620", Some(""), true),
        ["info", "620", "--extended"]
    );
}

#[test]
fn language_cycle_wraps_to_default() {
    assert_eq!(info::next_language(None).as_deref(), Some("english"));
    assert_eq!(
        info::next_language(Some("english")).as_deref(),
        Some("german")
    );
    let last = info::LANGUAGES.last().copied();
    assert_eq!(info::next_language(last), None);
    assert_eq!(info::next_language(Some("klingon")), None);
    assert_eq!(info::language_label(None), "default (Aurelia config)");
}

#[test]
fn steamdb_urls() {
    assert_eq!(info::steamdb_app_url(620), "https://steamdb.info/app/620/");
    assert_eq!(
        info::steamdb_depot_manifests_url(621),
        "https://steamdb.info/depot/621/manifests/"
    );
}

#[test]
fn actions_menu_offers_steamdb() {
    isolate_config();
    let browser = Browser::new(games());
    assert!(
        browser
            .action_candidates()
            .iter()
            .any(|r| r.action == Action::SteamDb && r.label == "Open on SteamDB")
    );
}

fn render(f: impl FnOnce(&mut tui::Frame<TestBackend>)) -> String {
    let mut terminal = Terminal::new(TestBackend::new(90, 30)).unwrap();
    terminal.draw(f).unwrap();
    let buffer = terminal.backend().buffer().clone();
    let mut text = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            text.push_str(&buffer.get(x, y).symbol);
        }
        text.push('\n');
    }
    text
}

#[test]
fn detail_pane_shows_playtime_flags_and_disk_size() {
    isolate_config();
    let game = games().remove(0);
    let text =
        render(|f| f.render_widget(ui::detail::detail(Some(&game), false, 60, 28), f.size()));
    assert!(text.contains("Playtime"), "{text}");
    assert!(text.contains("12h 34m"), "{text}");
    assert!(text.contains("queued"), "{text}");
    assert!(text.contains("13.42 GB"), "size_on_disk wins: {text}");
}

#[test]
fn list_badges_queued_and_online_games() {
    isolate_config();
    let browser = Browser::new(games());
    let text = render(|f| f.render_widget(ui::list::list(&browser, 90, 0), f.size()));
    let row = |name: &str| text.lines().find(|l| l.contains(name)).unwrap().to_string();
    assert!(row("Counter-Strike 2").contains("online"), "{text}");
    let portal2 = row("Portal 2");
    assert!(
        portal2.contains("queued") && portal2.contains("13.42 GB"),
        "{text}"
    );
    // Narrow lists drop the size.
    let portal2 = games().remove(0);
    let suffix = ui::library_info::list_suffix(&portal2, 30);
    assert_eq!(suffix.as_deref(), Some("queued"));
}

#[test]
fn extended_cache_is_per_app_and_language() {
    let ext: InfoJson =
        serde_json::from_str(include_str!("fixtures/library_info/info_extended.json")).unwrap();
    let ext = ext.extended.unwrap();
    assert!(info::claim_extended(9_620, Some("german")));
    assert!(!info::claim_extended(9_620, Some("german")), "fetch once");
    assert!(
        info::cached_extended(9_620, Some("german")).is_none(),
        "in flight"
    );
    info::store_extended(9_620, Some("german"), Some(ext));
    let hit = info::cached_extended(9_620, Some("german")).unwrap();
    assert_eq!(hit.metacritic, Some(95));
    // Other language: separate entry.
    assert!(info::cached_extended(9_620, None).is_none());
    assert!(info::claim_extended(9_620, None));
    // Failures stay cached, no retry.
    info::store_extended(9_620, None, None);
    assert!(!info::claim_extended(9_620, None));
}
