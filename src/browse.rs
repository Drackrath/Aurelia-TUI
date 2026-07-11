//! Library browsing model: the single source of truth for what the user is
//! looking at. Combines a tab filter (All/Installed/Updates/Favourites), a live
//! fuzzy text query, and a sort order, and owns the selection. Every browse
//! widget renders from this; the event loop only mutates it.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;

use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use tui::style::Style;
use tui::widgets::ListState;

use crate::config::Config;
use crate::interface::aurelia::{self, AccountJson, ConfigJson};
use crate::interface::game::Game;
use crate::interface::game_status::GameStatus;
use crate::theme;
use crate::util::error::STError;

/// The quick-filter tabs along the top of the library.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    All,
    Installed,
    Updates,
    Favourites,
}

impl Filter {
    /// Tab order, left to right.
    pub const TABS: [Filter; 4] = [
        Filter::All,
        Filter::Installed,
        Filter::Updates,
        Filter::Favourites,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Filter::All => "All",
            Filter::Installed => "Installed",
            Filter::Updates => "Updates",
            Filter::Favourites => "Favourites",
        }
    }

    pub fn index(self) -> usize {
        Filter::TABS.iter().position(|f| *f == self).unwrap_or(0)
    }
}

/// Which top-level view the tab bar is on. The four library filters all live in
/// the `Library` view; selecting the `Friends` tab (one slot to the right of
/// the filters) swaps the main list for the Friends & Chat panel and grants it
/// keyboard focus. The tab bar treats these as a single ring: the filters
/// occupy slots `0..TABS.len()` and Friends occupies the slot after them.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum View {
    Library,
    Friends,
}

/// Where a game's install currently sits, from the UI's point of view. Drives
/// whether the pause / resume / stop keys do anything for the selected game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallPhase {
    /// No UI-tracked install for this game.
    Idle,
    /// A download is in progress (pausable / stoppable).
    Active,
    /// The download was paused (resumable / stoppable).
    Paused,
}

/// Sort order for the visible list.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    Name,
    Installed,
}

impl Sort {
    pub fn label(self) -> &'static str {
        match self {
            Sort::Name => "name",
            Sort::Installed => "installed",
        }
    }

    pub fn next(self) -> Sort {
        match self {
            Sort::Name => Sort::Installed,
            Sort::Installed => Sort::Name,
        }
    }
}

/// Live tallies shown in the status bar (computed over the browsable universe,
/// i.e. after hidden/allowed-type filtering but independent of the active tab).
pub struct Counts {
    pub total: usize,
    pub visible: usize,
    pub installed: usize,
    pub updates: usize,
    pub downloading: usize,
}

/// What a game's status badge should look like.
pub struct Badge {
    pub glyph: &'static str,
    pub style: Style,
    /// Short trailing note (e.g. a download percentage or "update").
    pub note: Option<String>,
}

/// Classify a game into a status badge (glyph + colour + optional note).
pub fn badge(game: &Game) -> Badge {
    let status = game.get_status();
    let state = status.as_ref().map(|s| s.state.as_str()).unwrap_or("");

    if state.starts_with("paused") {
        // A paused download: freeze the last-seen percent next to a pause glyph.
        let note = state
            .split_whitespace()
            .find(|t| t.ends_with('%'))
            .map(|p| p.to_string());
        Badge {
            glyph: "⏸",
            style: Style::default().fg(theme::WARN),
            note,
        }
    } else if state.contains("downloading") || state.contains("processing") || state.contains("verifying") {
        // Pull a percentage out of e.g. "downloading 42.0%" if present.
        let note = state
            .split_whitespace()
            .find(|t| t.ends_with('%'))
            .map(|p| p.to_string());
        Badge {
            glyph: "⬇",
            style: Style::default().fg(theme::ACCENT_BRIGHT),
            note,
        }
    } else if state.contains("Failed") {
        Badge {
            glyph: "✖",
            style: theme::item_failed(),
            note: Some("failed".to_string()),
        }
    } else if state.contains("update") || game.update_available {
        Badge {
            glyph: "▲",
            style: Style::default().fg(theme::WARN),
            note: Some("update".to_string()),
        }
    } else if game.installed || state == "installed" || state.contains("Installed") || state.contains("ran") {
        Badge {
            glyph: "●",
            style: Style::default().fg(theme::ONLINE),
            note: None,
        }
    } else {
        Badge {
            glyph: "○",
            style: theme::dim(),
            note: None,
        }
    }
}

/// The browse state.
pub struct Browser {
    items: Vec<Game>,
    pub query: String,
    pub filter: Filter,
    pub sort: Sort,
    pub state: ListState,
    /// Whether the text filter is focused (typing edits the query).
    pub filtering: bool,
    /// Whether the help overlay is open.
    pub show_help: bool,
    /// Scroll offset (top row) within the help overlay.
    pub help_scroll: u16,
    /// Whether the Steam Cloud overlay is open.
    pub show_cloud: bool,
    /// Cloud files for the game the overlay is showing.
    pub cloud_files: Vec<aurelia::CloudFileJson>,
    /// A short status line for the cloud overlay (errors, sync progress).
    pub cloud_status: String,
    /// Whether the account overlay is open.
    pub show_account: bool,
    /// The fetched account details, shown by the account overlay.
    pub account_info: Option<AccountJson>,
    /// Whether the config (settings) overlay is open.
    pub show_config: bool,
    /// The fetched launcher configuration, shown by the config overlay.
    pub config_info: Option<ConfigJson>,
    /// The fetched network proxy setting (`aurelia config proxy`, v0.1.18), shown
    /// in the config overlay. Fetched alongside `config_info` when it opens.
    pub config_proxy: Option<aurelia::ProxyJson>,
    /// The inline proxy-URL edit buffer in the config overlay (None = not
    /// editing; Some = the URL the user is typing, empty to clear).
    pub config_proxy_input: Option<String>,
    /// Whether the Steam Wallet overlay is open.
    pub show_wallet: bool,
    /// The fetched wallet balance, shown by the wallet overlay.
    pub wallet_info: Option<aurelia::WalletJson>,
    /// Whether the description panel is expanded beyond its collapsed cap.
    pub expand_description: bool,
    /// Whether the achievements overlay is open.
    pub show_achievements: bool,
    /// The selected game's achievements (loaded when the overlay opens).
    pub achievements: Vec<aurelia::AchievementJson>,
    /// Scroll offset (top row) within the achievements overlay.
    pub ach_scroll: usize,
    /// The active top-level view. `Library` shows the game list under the four
    /// filter tabs; `Friends` swaps the list for the Friends & Chat panel (the
    /// fifth tab) and gives it keyboard focus (so j/k move the highlight and
    /// c/Enter/t act on the selected friend).
    pub view: View,
    /// The logged-in user's friends (loaded lazily the first time the panel is
    /// focused).
    pub friends: Vec<aurelia::FriendJson>,
    /// The highlighted row within the Friends panel. The widget derives its own
    /// scroll window from this so the highlight is always visible.
    pub friends_index: usize,
    /// Whether the add-friend (search/add) overlay is open.
    pub show_friend_add: bool,
    /// The reference the user is typing into the add-friend overlay (SteamID64 /
    /// profile URL / vanity name).
    pub friend_add_query: String,
    /// A short status line for the add-friend overlay (search/add progress, the
    /// resolved account, or an error). Shared with the worker thread that runs
    /// the (off-UI-thread) `friends search`/`friends add` calls.
    pub friend_add_status: Arc<Mutex<String>>,
    /// The account resolved by the last `friends search`, shown as a preview and
    /// used as the confirmation target for `friends add`. Shared with the worker
    /// thread that fills it in.
    pub friend_search_result: Option<aurelia::FriendSearchJson>,
    /// Resolved account published by an in-flight `friends search`, adopted into
    /// `friend_search_result` on the next poll (off-UI-thread handoff). Tagged
    /// with the search generation it was launched for so a stale (already-edited)
    /// worker's result can be dropped instead of adopted.
    friend_search_pending: Arc<Mutex<Option<(u64, aurelia::FriendSearchJson)>>>,
    /// Monotonic generation for the add-friend query. Bumped on every query edit
    /// and on overlay open/close; a search worker captures it at spawn and the
    /// poll only adopts a result whose tag still matches (closing the TOCTOU
    /// where a late worker could revive a preview for the edited query).
    friend_search_gen: u64,
    /// Whether the remove-friend confirmation prompt is open for the selection.
    pub confirm_friend_remove: bool,
    /// Set true by a friend add/remove worker on success so the next poll
    /// re-fetches the friends roster off the UI thread.
    friends_refresh_pending: Arc<Mutex<bool>>,
    /// A freshly fetched roster published by the refresh worker, adopted into
    /// `friends` on the next poll (off-UI-thread handoff).
    friends_roster_pending: Arc<Mutex<Option<Vec<aurelia::FriendJson>>>>,
    /// Whether the roster has been fetched at least once. Distinguishes a
    /// genuinely empty friends list from a not-yet-loaded one, so the lazy load
    /// on first entering the Friends tab fires exactly once.
    friends_loaded: bool,
    /// True while the initial (lazy) roster fetch is in flight, so the Friends
    /// panel can show "Loading friends…" instead of an empty/"no friends" state.
    pub friends_loading: bool,
    /// Whether the chat view is open.
    pub show_chat: bool,
    /// The messages in the open conversation (loaded when chat opens).
    pub chat_messages: Vec<aurelia::ChatMessageJson>,
    /// SteamID64 of the friend the chat view is talking to.
    pub chat_steamid: u64,
    /// Display name of the chat partner (shown in the title).
    pub chat_partner: String,
    /// The message the user is composing.
    pub chat_input: String,
    /// Scroll offset (top row) within the chat message list.
    pub chat_scroll: usize,
    /// Whether the inventory overlay is open.
    pub show_inventory: bool,
    /// The selected game's inventory items (loaded when the overlay opens).
    pub inventory: Vec<aurelia::InventoryItemJson>,
    /// Scroll offset (top row) within the inventory overlay.
    pub inv_scroll: usize,
    /// Whether the market listings overlay is open.
    pub show_market: bool,
    /// The account's active listings and buy orders (loaded when the overlay opens).
    pub market: Vec<aurelia::MarketListingJson>,
    /// Scroll offset (top row) within the market overlay.
    pub market_scroll: usize,
    /// Whether the Workshop overlay is open.
    pub show_workshop: bool,
    /// The selected game's Workshop items (loaded when the overlay opens).
    pub workshop: Vec<aurelia::WorkshopItemJson>,
    /// Scroll offset (top row) within the Workshop overlay.
    pub workshop_scroll: usize,
    /// The app id the Workshop overlay is showing (used for browse/subscribe).
    workshop_app_id: i32,
    /// Whether the overlay is in browse/search mode (vs. the subscribed list).
    pub workshop_browse: bool,
    /// The Workshop browse/search query the user is typing.
    pub workshop_query: String,
    /// Browse/search results (the current page from `workshop browse`).
    pub workshop_results: Vec<aurelia::WorkshopItemJson>,
    /// The highlighted row within the browse results list.
    pub workshop_index: usize,
    /// True while a browse/search request is in flight (drives a spinner line).
    pub workshop_searching: bool,
    /// A transient status line for the browse pane (subscribe/unsubscribe/error).
    pub workshop_status: String,
    /// Generation tag for browse requests: a worker's result is applied only if
    /// its tag still matches, so a slow earlier search never clobbers a newer one.
    workshop_gen: u64,
    /// Slot a browse worker posts its `(generation, result)` into; polled each
    /// loop iteration by `poll_workshop`.
    workshop_slot: Arc<Mutex<Option<(u64, Result<Vec<aurelia::WorkshopItemJson>, String>)>>>,
    /// True while a subscribe/unsubscribe request is in flight.
    pub workshop_acting: bool,
    /// Slot a subscribe/unsubscribe worker posts its outcome into:
    /// `(item_id, want_subscribed, Result<(), error>)`. Polled by `poll_workshop`.
    workshop_action_slot: Arc<Mutex<Option<(u64, bool, Result<(), String>)>>>,
    /// Slot a subscribe/unsubscribe worker posts a re-fetched subscribed list
    /// into on success: `(generation, items)`. Drained (non-blocking) by
    /// `poll_workshop` and installed only if the generation still matches, so a
    /// stale refresh can never clobber newer overlay state. This keeps the
    /// post-action `workshop list` shell-out entirely off the render thread.
    workshop_refresh_slot: Arc<Mutex<Option<(u64, Vec<aurelia::WorkshopItemJson>)>>>,
    /// True while a rate (thumbs up/down) request is in flight.
    pub workshop_rating: bool,
    /// Slot a rate worker posts its outcome into: `(generation, Result<(), error>)`.
    /// Drained by `poll_workshop`; a stale-generation result (the overlay closed
    /// or moved on) is dropped rather than shown.
    workshop_rate_slot: Arc<Mutex<Option<(u64, Result<(), String>)>>>,
    /// Whether the comments sub-pane is open (over the browse results).
    pub workshop_comments_open: bool,
    /// True while a comments fetch is in flight (drives a spinner line).
    pub workshop_comments_loading: bool,
    /// The fetched comments for the item the sub-pane is showing.
    pub workshop_comments: Vec<aurelia::WorkshopCommentJson>,
    /// Scroll offset (top row) within the comments sub-pane.
    pub workshop_comments_scroll: usize,
    /// A transient status line for the comments sub-pane (errors / empty).
    pub workshop_comments_status: String,
    /// The id of the item the comments sub-pane was opened for (shown in title).
    workshop_comments_id: u64,
    /// Slot a comments worker posts its `(generation, result)` into; drained by
    /// `poll_workshop` and applied only if the generation still matches, so a
    /// slow fetch for a since-closed/superseded sub-pane is discarded.
    workshop_comments_slot:
        Arc<Mutex<Option<(u64, Result<Vec<aurelia::WorkshopCommentJson>, String>)>>>,
    /// Whether the uninstall confirmation prompt is open for the selection.
    pub confirm_uninstall: bool,
    /// Whether the DLC overlay is open.
    pub show_dlc: bool,
    /// DLC for the game the overlay was opened for.
    pub dlc: Vec<aurelia::DlcJson>,
    /// The highlighted row within the DLC overlay.
    pub dlc_index: usize,
    /// The base app id the DLC overlay is showing (for re-fetching after a toggle).
    dlc_app_id: i32,
    /// Whether the branches overlay is open.
    pub show_branches: bool,
    /// Beta branches for the game the overlay was opened for.
    pub branches: Vec<aurelia::BranchJson>,
    /// The highlighted row within the branches overlay.
    pub branch_index: usize,
    /// The app id the branches overlay is showing (used when switching branch).
    branch_app_id: i32,
    /// Whether the depots overlay is open.
    pub show_depots: bool,
    /// Depots for the game the overlay was opened for.
    pub depots: Vec<aurelia::DepotJson>,
    /// Scroll offset (top row) within the depots overlay.
    pub depots_scroll: usize,
    /// Whether the launch-options overlay is open.
    pub show_launch: bool,
    /// The selected game's launch options (loaded when the overlay opens).
    pub launch_options: Vec<aurelia::LaunchOptionJson>,
    /// Scroll offset (top row) within the launch-options overlay.
    pub launch_scroll: usize,
    /// Whether the move (relocate install) prompt is open.
    pub show_move: bool,
    /// The destination library path the user is typing.
    pub move_path: String,
    /// The app id being moved.
    pub move_app_id: i32,
    /// A short status line for the move prompt (progress, errors).
    pub move_status: String,
    /// Whether the relink (relink install) prompt is open.
    pub show_relink: bool,
    /// The destination library path the user is typing.
    pub relink_path: String,
    /// The app id being relinked.
    pub relink_app_id: i32,
    /// A short status line for the relink prompt (progress, errors).
    pub relink_status: String,
    /// Whether the import (register existing install) prompt is open.
    pub show_import: bool,
    /// The library path the user is typing.
    pub import_path: String,
    /// The app id being imported.
    pub import_app_id: i32,
    /// A short status line for the import prompt (progress, errors).
    pub import_status: String,
    /// Whether the Proton runtimes overlay is open.
    pub show_proton: bool,
    /// The Proton/Wine runtimes (loaded when the overlay opens).
    pub protons: Vec<aurelia::ProtonJson>,
    /// The highlighted row within the Proton overlay.
    pub proton_index: usize,
    /// Shared status cell for an in-flight Proton install/uninstall, streamed
    /// from the backend worker thread (mirrors a game's install status).
    pub proton_status: Arc<Mutex<Option<GameStatus>>>,
    /// Whether the Proton uninstall confirmation prompt is showing.
    pub confirm_proton_uninstall: bool,
    /// Whether the running-games overlay is open.
    pub show_running: bool,
    /// The games Aurelia currently has running (loaded when the overlay opens).
    pub running: Vec<aurelia::RunningJson>,
    /// The highlighted row within the running overlay.
    pub running_index: usize,
    /// Whether the Community Market search overlay is open.
    pub show_market_search: bool,
    /// The query the user is typing into the market search overlay.
    pub market_query: String,
    /// The resolved search results (adopted from the worker cell on each poll).
    pub market_results: Vec<aurelia::MarketSearchResultJson>,
    /// The highlighted row within the search results.
    pub market_results_index: usize,
    /// Scroll offset (top row) within the search-results list.
    pub market_results_scroll: usize,
    /// A short status line for the search overlay (progress, errors, price).
    pub market_search_status: String,
    /// Generation tag for market searches. Bumped whenever the query changes or
    /// the search overlay opens/closes; captured at worker spawn and re-checked
    /// in [`Browser::poll_market`] so a slow OLDER search can't clobber a NEWER
    /// one's results when Enter is hit in rapid succession (mirrors
    /// `friend_search_gen`).
    market_search_gen: u64,
    /// Shared cell the background search worker publishes its (gen-tagged) result
    /// into.
    market_search_cell: Arc<Mutex<(u64, aurelia::MarketSearchState)>>,
    /// Shared cell the background price worker publishes its result into.
    market_price_cell: Arc<Mutex<aurelia::MarketPriceState>>,
    /// Per-game install controls, keyed by app id. An entry is created when the
    /// UI starts an install and lets it pause/stop the in-flight download.
    install_controls: HashMap<i32, aurelia::InstallControl>,
    /// The library folder each in-flight install was started in, keyed by app
    /// id, so a paused install resumes into the *same* library (not the default).
    install_library_choice: HashMap<i32, Option<String>>,
    /// Whether the "game not installed — install now?" prompt is open (shown
    /// when the user tries to launch a game the listing marks not installed).
    pub confirm_install: bool,
    /// Whether the install-location picker (choose which Steam library folder to
    /// install into) is open.
    pub show_install_picker: bool,
    /// The available library folders shown in the picker (roots from `aurelia
    /// libraries`, each with its drive's free space).
    pub install_libraries: Vec<aurelia::LibraryJson>,
    /// Estimated on-disk size of the game being installed, for the picker to
    /// show and to gauge whether a library has room. `None` if unknown.
    pub install_estimate: Option<u64>,
    /// The highlighted row within the install picker.
    pub install_picker_index: usize,
    /// Off-thread inbox for targeted single-game install-state refreshes (after
    /// an install completes or is cancelled). A worker fetches the fresh `list`
    /// entry and drops it here; [`Browser::poll_game_refresh`] adopts it,
    /// patching just that game's `installed`/update flags — no full reload.
    game_refresh_slot: Arc<Mutex<Vec<aurelia::LibraryGameJson>>>,
    /// A transient one-line notice (e.g. an action that failed) shown in the
    /// status bar until the next keypress. Keeps a failed backend call from
    /// having to crash the whole TUI to report itself.
    pub notice: Option<String>,

    // --- Actions menu (per-game command palette) ---
    /// Whether the per-game Actions menu is open. This is the primary way to
    /// reach the many per-game actions without memorising 40 hotkeys; the base
    /// keymap keeps single-key accelerators for power users.
    pub show_actions: bool,
    /// The highlighted row within the *filtered* Actions menu.
    pub actions_index: usize,
    /// The live filter query typed into the Actions menu (empty = show all).
    pub actions_filter: String,
    /// The app id the Actions menu was opened for.
    actions_app_id: i32,

    // --- Versions & pinning overlay ---
    /// Whether the versions/pinning overlay is open.
    pub show_versions: bool,
    /// Per-depot current manifest ids for the game (from `aurelia manifests`).
    pub versions_manifests: Vec<aurelia::DepotManifestInfo>,
    /// Install + pin state for the game (from `aurelia available`): the pin flag
    /// and the pinned depot→manifest map.
    pub versions_available: Option<aurelia::AvailableJson>,
    /// The highlighted depot row within the versions overlay.
    pub versions_index: usize,
    /// The app id the versions overlay is showing.
    versions_app_id: i32,
    /// A transient status line for the versions overlay (pin/unpin/downgrade).
    pub versions_status: String,
    /// When `Some`, the overlay is prompting for a manifest id to downgrade the
    /// highlighted depot to (older ids come from SteamDB); the buffer is what the
    /// user has typed so far.
    pub versions_input: Option<String>,
    /// Shared status cell for an in-flight downgrade, streamed from the backend
    /// worker thread (mirrors a game's install status).
    pub versions_busy: Arc<Mutex<Option<GameStatus>>>,

    // --- Game settings overlay (per-game config overrides + launch script) ---
    /// Whether the per-game settings overlay is open.
    pub show_game_config: bool,
    /// The game's current config overrides (from `aurelia config game`).
    pub game_config: Option<aurelia::GameConfigJson>,
    /// The game's launch-script state (from `aurelia scripts show`).
    pub game_script: Option<aurelia::ScriptShowJson>,
    /// Installed Proton runtime names, for cycling the per-game forced runtime.
    pub game_config_protons: Vec<String>,
    /// The highlighted settings row.
    pub game_config_index: usize,
    /// The app id the settings overlay is showing.
    game_config_app_id: i32,
    /// A transient status line for the settings overlay.
    pub game_config_status: String,

    // --- Collections overlay ---
    /// Whether the collections overlay is open.
    pub show_collections: bool,
    /// The account's Steam library collections (from `aurelia collections list`).
    pub collections: Vec<aurelia::CollectionJson>,
    /// The highlighted collection row.
    pub collections_index: usize,
    /// A transient status line for the collections overlay.
    pub collections_status: String,
    /// The game the overlay adds to / removes from the highlighted collection.
    collections_app_id: i32,
    /// The inline text-entry buffer for creating a collection (None = not
    /// prompting).
    pub collections_input: Option<String>,

    // --- Runtime plugins overlay (umu / luxtorpeda / steam-runtime) ---
    /// Whether the runtime-plugins overlay is open.
    pub show_engine: bool,
    /// umu-launcher plugin status.
    pub engine_umu: Option<aurelia::PluginStatusJson>,
    /// luxtorpeda plugin status.
    pub engine_lux: Option<aurelia::PluginStatusJson>,
    /// Steam Linux Runtime status.
    pub engine_steam_runtime: Option<aurelia::SteamRuntimeStatusJson>,
    /// The highlighted plugin row (0 = umu, 1 = luxtorpeda, 2 = steam-runtime).
    pub engine_index: usize,
    /// A transient status line for the plugins overlay.
    pub engine_status: String,
    /// Shared cell for an in-flight plugin install/update/repair, run on a worker
    /// thread so the long network download doesn't freeze the TUI (mirrors
    /// `proton_status` / `versions_busy`). Holds `"working…"` while running, then
    /// a terminal `"Done."`/`"Failed: …"` that [`Browser::poll_engine`] adopts.
    pub engine_busy: Arc<Mutex<Option<GameStatus>>>,
}

/// A single entry in the per-game Actions menu. Selecting one runs the same
/// code path as its base-keymap accelerator (see the `Mode::Browse` dispatch in
/// `main.rs`), so the menu and the hotkeys never drift apart.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Play,
    Install,
    PauseResume,
    CancelInstall,
    Update,
    Verify,
    Uninstall,
    Versions,
    Branches,
    GameSettings,
    Proton,
    Engine,
    LaunchOptions,
    Dlc,
    Workshop,
    Cloud,
    Achievements,
    Depots,
    Inventory,
    Move,
    Relink,
    Import,
    Collections,
    Favourite,
    Hide,
}

/// One rendered row of the Actions menu: the action, the category header it
/// sits under, its human label, and the base-keymap key that also triggers it
/// (shown as a hint; empty when the action is reachable only from the menu).
pub struct ActionRow {
    pub action: Action,
    pub category: &'static str,
    pub label: &'static str,
    pub key: &'static str,
}

impl Browser {
    pub fn new(items: Vec<Game>) -> Browser {
        let mut browser = Browser {
            items,
            query: String::new(),
            filter: Filter::All,
            sort: Sort::Name,
            state: ListState::default(),
            filtering: false,
            show_help: false,
            help_scroll: 0,
            show_cloud: false,
            cloud_files: Vec::new(),
            cloud_status: String::new(),
            show_account: false,
            account_info: None,
            show_config: false,
            config_info: None,
            config_proxy: None,
            config_proxy_input: None,
            show_wallet: false,
            wallet_info: None,
            expand_description: false,
            show_achievements: false,
            achievements: Vec::new(),
            ach_scroll: 0,
            view: View::Library,
            friends: Vec::new(),
            friends_index: 0,
            show_friend_add: false,
            friend_add_query: String::new(),
            friend_add_status: Arc::new(Mutex::new(String::new())),
            friend_search_result: None,
            friend_search_pending: Arc::new(Mutex::new(None)),
            friend_search_gen: 0,
            confirm_friend_remove: false,
            friends_refresh_pending: Arc::new(Mutex::new(false)),
            friends_roster_pending: Arc::new(Mutex::new(None)),
            friends_loaded: false,
            friends_loading: false,
            show_chat: false,
            chat_messages: Vec::new(),
            chat_steamid: 0,
            chat_partner: String::new(),
            chat_input: String::new(),
            chat_scroll: 0,
            show_inventory: false,
            inventory: Vec::new(),
            inv_scroll: 0,
            show_market: false,
            market: Vec::new(),
            market_scroll: 0,
            show_workshop: false,
            workshop: Vec::new(),
            workshop_scroll: 0,
            workshop_app_id: 0,
            workshop_browse: false,
            workshop_query: String::new(),
            workshop_results: Vec::new(),
            workshop_index: 0,
            workshop_searching: false,
            workshop_status: String::new(),
            workshop_gen: 0,
            workshop_slot: Arc::new(Mutex::new(None)),
            workshop_acting: false,
            workshop_action_slot: Arc::new(Mutex::new(None)),
            workshop_refresh_slot: Arc::new(Mutex::new(None)),
            workshop_rating: false,
            workshop_rate_slot: Arc::new(Mutex::new(None)),
            workshop_comments_open: false,
            workshop_comments_loading: false,
            workshop_comments: Vec::new(),
            workshop_comments_scroll: 0,
            workshop_comments_status: String::new(),
            workshop_comments_id: 0,
            workshop_comments_slot: Arc::new(Mutex::new(None)),
            confirm_uninstall: false,
            show_dlc: false,
            dlc: Vec::new(),
            dlc_index: 0,
            dlc_app_id: 0,
            show_branches: false,
            branches: Vec::new(),
            branch_index: 0,
            branch_app_id: 0,
            show_depots: false,
            depots: Vec::new(),
            depots_scroll: 0,
            show_launch: false,
            launch_options: Vec::new(),
            launch_scroll: 0,
            show_move: false,
            move_path: String::new(),
            move_app_id: 0,
            move_status: String::new(),
            show_relink: false,
            relink_path: String::new(),
            relink_app_id: 0,
            relink_status: String::new(),
            show_import: false,
            import_path: String::new(),
            import_app_id: 0,
            import_status: String::new(),
            show_proton: false,
            protons: Vec::new(),
            proton_index: 0,
            proton_status: Arc::new(Mutex::new(None)),
            confirm_proton_uninstall: false,
            show_running: false,
            running: Vec::new(),
            running_index: 0,
            show_market_search: false,
            market_query: String::new(),
            market_results: Vec::new(),
            market_results_index: 0,
            market_results_scroll: 0,
            market_search_status: String::new(),
            market_search_gen: 0,
            market_search_cell: Arc::new(Mutex::new((0, aurelia::MarketSearchState::Idle))),
            market_price_cell: Arc::new(Mutex::new(aurelia::MarketPriceState::Idle)),
            install_controls: HashMap::new(),
            install_library_choice: HashMap::new(),
            confirm_install: false,
            show_install_picker: false,
            install_libraries: Vec::new(),
            install_estimate: None,
            install_picker_index: 0,
            game_refresh_slot: Arc::new(Mutex::new(Vec::new())),
            notice: None,
            show_actions: false,
            actions_index: 0,
            actions_filter: String::new(),
            actions_app_id: 0,
            show_versions: false,
            versions_manifests: Vec::new(),
            versions_available: None,
            versions_index: 0,
            versions_app_id: 0,
            versions_status: String::new(),
            versions_input: None,
            versions_busy: Arc::new(Mutex::new(None)),
            show_game_config: false,
            game_config: None,
            game_script: None,
            game_config_protons: Vec::new(),
            game_config_index: 0,
            game_config_app_id: 0,
            game_config_status: String::new(),
            show_collections: false,
            collections: Vec::new(),
            collections_index: 0,
            collections_status: String::new(),
            collections_app_id: 0,
            collections_input: None,
            show_engine: false,
            engine_umu: None,
            engine_lux: None,
            engine_steam_runtime: None,
            engine_index: 0,
            engine_status: String::new(),
            engine_busy: Arc::new(Mutex::new(None)),
        };
        browser.reset_selection();
        browser
    }

    // --- DLC overlay ---

    /// Fetch the DLC for `app_id` and open the overlay. Blocks on the `aurelia
    /// dlc` subprocess; the selection resets to the first row.
    pub fn open_dlc(&mut self, app_id: i32) -> Result<(), STError> {
        self.dlc = aurelia::dlc(app_id)?;
        self.dlc_app_id = app_id;
        self.dlc_index = 0;
        self.show_dlc = true;
        Ok(())
    }

    /// Close the DLC overlay and drop its contents.
    pub fn close_dlc(&mut self) {
        self.show_dlc = false;
        self.dlc.clear();
        self.dlc_index = 0;
    }

    /// The highlighted DLC entry, if any.
    pub fn selected_dlc(&self) -> Option<&aurelia::DlcJson> {
        self.dlc.get(self.dlc_index)
    }

    pub fn dlc_next(&mut self) {
        if self.dlc.is_empty() {
            self.dlc_index = 0;
            return;
        }
        self.dlc_index = (self.dlc_index + 1) % self.dlc.len();
    }

    pub fn dlc_previous(&mut self) {
        if self.dlc.is_empty() {
            self.dlc_index = 0;
            return;
        }
        self.dlc_index = if self.dlc_index == 0 {
            self.dlc.len() - 1
        } else {
            self.dlc_index - 1
        };
    }

    /// Re-fetch the open overlay's DLC after a toggle, keeping the selection in
    /// range.
    pub fn refresh_dlc(&mut self) -> Result<(), STError> {
        self.dlc = aurelia::dlc(self.dlc_app_id)?;
        if self.dlc.is_empty() {
            self.dlc_index = 0;
        } else if self.dlc_index >= self.dlc.len() {
            self.dlc_index = self.dlc.len() - 1;
        }
        Ok(())
    }

    // --- Branches overlay ---

    /// Fetch the beta branches for `app_id` and open the overlay. Blocks on the
    /// `aurelia branches` subprocess; the selection resets to the first row.
    pub fn open_branches(&mut self, app_id: i32) -> Result<(), STError> {
        self.branches = aurelia::branches(app_id)?;
        self.branch_app_id = app_id;
        self.branch_index = 0;
        self.show_branches = true;
        Ok(())
    }

    /// Close the branches overlay and drop its contents.
    pub fn close_branches(&mut self) {
        self.show_branches = false;
        self.branches.clear();
        self.branch_index = 0;
    }

    /// The app id the branches overlay was opened for.
    pub fn branch_app_id(&self) -> i32 {
        self.branch_app_id
    }

    /// The highlighted branch entry, if any.
    pub fn selected_branch(&self) -> Option<&aurelia::BranchJson> {
        self.branches.get(self.branch_index)
    }

    pub fn branch_next(&mut self) {
        if self.branches.is_empty() {
            self.branch_index = 0;
            return;
        }
        self.branch_index = (self.branch_index + 1) % self.branches.len();
    }

    pub fn branch_previous(&mut self) {
        if self.branches.is_empty() {
            self.branch_index = 0;
            return;
        }
        self.branch_index = if self.branch_index == 0 {
            self.branches.len() - 1
        } else {
            self.branch_index - 1
        };
    }

    // --- Depots overlay ---

    /// Fetch the depots for `app_id` (blocking) and open the overlay. A fetch
    /// error simply opens an empty overlay ("No depots.").
    pub fn open_depots(&mut self, app_id: i32) {
        self.depots = aurelia::depots(app_id).unwrap_or_default();
        self.depots_scroll = 0;
        self.show_depots = true;
    }

    /// Close the depots overlay and drop its contents.
    pub fn close_depots(&mut self) {
        self.show_depots = false;
        self.depots.clear();
        self.depots_scroll = 0;
    }

    /// Scroll the depots overlay down by one row (clamped).
    pub fn depots_scroll_down(&mut self) {
        let max = self.depots.len().saturating_sub(1);
        if self.depots_scroll < max {
            self.depots_scroll += 1;
        }
    }

    /// Scroll the depots overlay up by one row (clamped).
    pub fn depots_scroll_up(&mut self) {
        self.depots_scroll = self.depots_scroll.saturating_sub(1);
    }

    // --- Move (relocate install) prompt ---

    /// Open the move prompt for `app_id`, clearing any previous path/status.
    pub fn open_move(&mut self, app_id: i32) {
        self.move_app_id = app_id;
        self.move_path.clear();
        self.move_status.clear();
        self.show_move = true;
    }

    /// Close the move prompt and drop its state.
    pub fn close_move(&mut self) {
        self.show_move = false;
        self.move_path.clear();
        self.move_status.clear();
        self.move_app_id = 0;
    }

    /// Append a typed character to the destination path.
    pub fn move_push(&mut self, c: char) {
        self.move_path.push(c);
    }

    /// Remove the last character from the destination path.
    pub fn move_pop(&mut self) {
        self.move_path.pop();
    }

    /// Relocate the game to the typed library folder (blocking on `aurelia
    /// move`). Updates `move_status` to reflect progress/outcome and returns the
    /// backend result.
    pub fn do_move(&mut self) -> Result<(), STError> {
        self.move_status = "moving...".to_string();
        match aurelia::move_game(self.move_app_id, &self.move_path) {
            Ok(()) => {
                self.move_status = "done".to_string();
                Ok(())
            }
            Err(err) => {
                self.move_status = format!("Failed: {}", err);
                Err(err)
            }
        }
    }

    // --- Relink (relink install) prompt ---

    /// Open the relink prompt for `app_id`, clearing any previous path/status.
    pub fn open_relink(&mut self, app_id: i32) {
        self.relink_app_id = app_id;
        self.relink_path.clear();
        self.relink_status.clear();
        self.show_relink = true;
    }

    /// Close the relink prompt and drop its state.
    pub fn close_relink(&mut self) {
        self.show_relink = false;
        self.relink_path.clear();
        self.relink_status.clear();
        self.relink_app_id = 0;
    }

    /// Append a typed character to the destination path.
    pub fn relink_push(&mut self, c: char) {
        self.relink_path.push(c);
    }

    /// Remove the last character from the destination path.
    pub fn relink_pop(&mut self) {
        self.relink_path.pop();
    }

    /// Relink the game to the typed library folder (blocking on `aurelia
    /// relink`). Updates `relink_status` to reflect progress/outcome and returns
    /// the backend result.
    pub fn do_relink(&mut self) -> Result<(), STError> {
        self.relink_status = "relinking...".to_string();
        match aurelia::relink(self.relink_app_id, &self.relink_path) {
            Ok(()) => {
                self.relink_status = "done".to_string();
                Ok(())
            }
            Err(err) => {
                self.relink_status = format!("Failed: {}", err);
                Err(err)
            }
        }
    }

    // --- Import (register existing install) prompt ---

    /// Open the import prompt for `app_id`, clearing any previous path/status.
    pub fn open_import(&mut self, app_id: i32) {
        self.import_app_id = app_id;
        self.import_path.clear();
        self.import_status.clear();
        self.show_import = true;
    }

    /// Close the import prompt and drop its state.
    pub fn close_import(&mut self) {
        self.show_import = false;
        self.import_path.clear();
        self.import_status.clear();
        self.import_app_id = 0;
    }

    /// Append a typed character to the library path.
    pub fn import_push(&mut self, c: char) {
        self.import_path.push(c);
    }

    /// Remove the last character from the library path.
    pub fn import_pop(&mut self) {
        self.import_path.pop();
    }

    /// Register the on-disk install at the typed library folder (blocking on
    /// `aurelia import`). Updates `import_status` to reflect progress/outcome and
    /// returns the backend result.
    pub fn do_import(&mut self) -> Result<(), STError> {
        self.import_status = "importing...".to_string();
        match aurelia::import_game(self.import_app_id, &self.import_path) {
            Ok(()) => {
                self.import_status = "done".to_string();
                Ok(())
            }
            Err(err) => {
                self.import_status = format!("Failed: {}", err);
                Err(err)
            }
        }
    }

    // --- Proton overlay ---

    /// Fetch the Proton/Wine runtimes and open the overlay. Blocks on the
    /// `aurelia proton list` subprocess; the selection resets to the first row.
    pub fn open_proton(&mut self) -> Result<(), STError> {
        self.protons = aurelia::proton_list()?;
        self.proton_index = 0;
        self.show_proton = true;
        Ok(())
    }

    /// Close the Proton overlay and drop its contents.
    pub fn close_proton(&mut self) {
        self.show_proton = false;
        self.confirm_proton_uninstall = false;
        self.protons.clear();
        self.proton_index = 0;
    }

    /// Re-fetch the Proton runtimes (e.g. after an install/uninstall) while
    /// keeping the current row highlighted. A fetch error leaves the list as-is.
    pub fn refresh_proton(&mut self) {
        if let Ok(protons) = aurelia::proton_list() {
            self.protons = protons;
            self.proton_index = self
                .proton_index
                .min(self.protons.len().saturating_sub(1));
        }
    }

    /// The current Proton install/uninstall status line, if any work is in
    /// flight or just finished (cloned out of the shared status cell).
    pub fn proton_status_line(&self) -> Option<String> {
        self.proton_status
            .lock()
            .ok()
            .and_then(|s| s.as_ref().map(|st| st.state.clone()))
    }

    /// Drop any leftover Proton status line.
    pub fn clear_proton_status(&self) {
        if let Ok(mut guard) = self.proton_status.lock() {
            *guard = None;
        }
    }

    /// Whether the highlighted runtime can be uninstalled (an installed custom
    /// GE runtime). Drives whether the uninstall key/confirm is offered.
    pub fn selected_proton_uninstallable(&self) -> bool {
        self.selected_proton()
            .map(|p| p.uninstallable())
            .unwrap_or(false)
    }

    /// Uninstall the highlighted custom (GE) runtime and refresh the list. A
    /// no-op unless the highlighted runtime is uninstallable.
    pub fn do_proton_uninstall(&mut self) -> Result<(), STError> {
        let name = match self.selected_proton() {
            Some(p) if p.uninstallable() => p.name.clone(),
            _ => return Ok(()),
        };
        aurelia::proton_uninstall(&name)?;
        self.refresh_proton();
        Ok(())
    }

    /// The highlighted Proton runtime, if any.
    pub fn selected_proton(&self) -> Option<&aurelia::ProtonJson> {
        self.protons.get(self.proton_index)
    }

    pub fn proton_next(&mut self) {
        if self.protons.is_empty() {
            self.proton_index = 0;
            return;
        }
        self.proton_index = (self.proton_index + 1) % self.protons.len();
    }

    pub fn proton_previous(&mut self) {
        if self.protons.is_empty() {
            self.proton_index = 0;
            return;
        }
        self.proton_index = if self.proton_index == 0 {
            self.protons.len() - 1
        } else {
            self.proton_index - 1
        };
    }

    // --- Running overlay ---

    /// Fetch the games Aurelia currently has running (blocking) and open the
    /// overlay. A fetch error simply opens an empty overlay ("No games
    /// running."); the selection resets to the first row.
    pub fn open_running(&mut self) {
        self.running = aurelia::running().unwrap_or_default();
        self.running_index = 0;
        self.show_running = true;
    }

    /// Close the running overlay and drop its contents.
    pub fn close_running(&mut self) {
        self.show_running = false;
        self.running.clear();
        self.running_index = 0;
    }

    /// The highlighted running game, if any.
    pub fn selected_running(&self) -> Option<&aurelia::RunningJson> {
        self.running.get(self.running_index)
    }

    pub fn running_next(&mut self) {
        if self.running.is_empty() {
            self.running_index = 0;
            return;
        }
        self.running_index = (self.running_index + 1) % self.running.len();
    }

    pub fn running_previous(&mut self) {
        if self.running.is_empty() {
            self.running_index = 0;
            return;
        }
        self.running_index = if self.running_index == 0 {
            self.running.len() - 1
        } else {
            self.running_index - 1
        };
    }

    /// Re-fetch the running games after a stop, keeping the selection in range.
    pub fn refresh_running(&mut self) {
        self.running = aurelia::running().unwrap_or_default();
        if self.running.is_empty() {
            self.running_index = 0;
        } else if self.running_index >= self.running.len() {
            self.running_index = self.running.len() - 1;
        }
    }

    /// Toggle the expanded/collapsed state of the description panel.
    pub fn toggle_description(&mut self) {
        self.expand_description = !self.expand_description;
    }

    /// Fetch the Steam Cloud file list for `app_id` (blocking) and open the
    /// overlay. On failure the overlay still opens, showing the error.
    pub fn open_cloud(&mut self, app_id: i32) {
        self.show_cloud = true;
        self.refresh_cloud(app_id);
    }

    /// Re-fetch the cloud file list for `app_id`, updating the status line.
    pub fn refresh_cloud(&mut self, app_id: i32) {
        match aurelia::cloud_list(app_id) {
            Ok(files) => {
                self.cloud_files = files;
                self.cloud_status.clear();
            }
            Err(err) => {
                self.cloud_files.clear();
                self.cloud_status = format!("Failed: {}", err);
            }
        }
    }

    /// Sync the game's Steam Cloud saves in `direction` (blocking), then re-fetch
    /// the list. `Both` syncs down then up; `Down`/`Up` restrict the transfer.
    pub fn sync_cloud(&mut self, app_id: i32, direction: aurelia::CloudDirection) {
        self.cloud_status = "syncing...".to_string();
        if let Err(err) = aurelia::cloud_sync(app_id, direction) {
            self.cloud_status = format!("Failed: {}", err);
            return;
        }
        self.refresh_cloud(app_id);
        if self.cloud_status.is_empty() {
            self.cloud_status = match direction {
                aurelia::CloudDirection::Both => "synced",
                aurelia::CloudDirection::Down => "downloaded",
                aurelia::CloudDirection::Up => "uploaded",
            }
            .to_string();
        }
    }

    /// Close the Steam Cloud overlay and drop its state.
    pub fn close_cloud(&mut self) {
        self.show_cloud = false;
        self.cloud_files.clear();
        self.cloud_status.clear();
    }

    /// Fetch the selected game's achievements (blocking) and open the overlay.
    /// A fetch error simply opens an empty overlay ("No achievements.").
    pub fn open_achievements(&mut self) {
        let Some(game) = self.selected() else {
            return;
        };
        self.achievements = aurelia::achievements(game.id).unwrap_or_default();
        self.ach_scroll = 0;
        self.show_achievements = true;
    }

    /// Close the achievements overlay and drop its data.
    pub fn close_achievements(&mut self) {
        self.show_achievements = false;
        self.achievements = Vec::new();
        self.ach_scroll = 0;
    }

    /// Scroll the achievements overlay down by one row (clamped).
    pub fn ach_scroll_down(&mut self) {
        let max = self.achievements.len().saturating_sub(1);
        if self.ach_scroll < max {
            self.ach_scroll += 1;
        }
    }

    /// Scroll the achievements overlay up by one row (clamped).
    pub fn ach_scroll_up(&mut self) {
        self.ach_scroll = self.ach_scroll.saturating_sub(1);
    }

    /// Scroll the help overlay down by one row (clamped to the last row).
    pub fn help_scroll_down(&mut self) {
        let max = crate::ui::help::row_count().saturating_sub(1);
        if self.help_scroll < max {
            self.help_scroll += 1;
        }
    }

    /// Scroll the help overlay up by one row (clamped).
    pub fn help_scroll_up(&mut self) {
        self.help_scroll = self.help_scroll.saturating_sub(1);
    }

    /// Whether the Friends tab is the active view (so the Friends & Chat panel
    /// holds keyboard focus). Derived from the view — the Friends tab *is* the
    /// focus, so there is no separate focus flag to drift out of sync.
    pub fn friends_focused(&self) -> bool {
        self.view == View::Friends
    }

    /// Enter the Friends view (select the Friends tab). The tab is shown
    /// immediately; the roster is lazy-loaded off the UI thread the first time
    /// (the panel shows "Loading friends…" until [`Browser::poll_friends_ops`]
    /// adopts the result), so tabbing in never blocks/lags. Idempotent:
    /// re-entering keeps the already-loaded list.
    pub fn enter_friends(&mut self) {
        self.view = View::Friends;
        if !self.friends_loaded && !self.friends_loading {
            self.friends_index = 0;
            self.friends_loading = true;
            // Reuse the existing off-thread refresh path: flag it and the poll
            // loop spawns the worker and adopts the roster when it lands.
            if let Ok(mut flag) = self.friends_refresh_pending.lock() {
                *flag = true;
            }
        }
    }

    /// Toggle the Friends view on/off. Entering loads the roster (see
    /// [`Browser::enter_friends`]); leaving returns to the Library view (the
    /// active filter is preserved) while keeping the loaded list.
    pub fn toggle_friends_focus(&mut self) {
        if self.view == View::Friends {
            self.view = View::Library;
        } else {
            self.enter_friends();
        }
    }

    /// Leave the Friends view (Esc), returning to the Library list while keeping
    /// the loaded friends list.
    pub fn unfocus_friends(&mut self) {
        self.view = View::Library;
    }

    /// The highlighted friend, if any.
    pub fn selected_friend(&self) -> Option<&aurelia::FriendJson> {
        self.friends.get(self.friends_index)
    }

    /// Move the friends highlight down by one row (clamped). The widget scrolls
    /// the whole list to keep the highlight visible.
    pub fn friends_scroll_down(&mut self) {
        if self.friends.is_empty() {
            self.friends_index = 0;
            return;
        }
        if self.friends_index + 1 < self.friends.len() {
            self.friends_index += 1;
        }
    }

    /// Move the friends highlight up by one row (clamped).
    pub fn friends_scroll_up(&mut self) {
        self.friends_index = self.friends_index.saturating_sub(1);
    }

    // --- Friend management (search / add / remove) ---

    /// Open the add-friend (search/add) overlay, clearing any previous query,
    /// resolved preview, and status.
    pub fn open_friend_add(&mut self) {
        self.show_friend_add = true;
        self.friend_add_query.clear();
        self.friend_search_result = None;
        self.friend_search_gen = self.friend_search_gen.wrapping_add(1);
        if let Ok(mut status) = self.friend_add_status.lock() {
            status.clear();
        }
        if let Ok(mut pending) = self.friend_search_pending.lock() {
            *pending = None;
        }
    }

    /// Close the add-friend overlay and drop its contents.
    pub fn close_friend_add(&mut self) {
        self.show_friend_add = false;
        self.friend_add_query.clear();
        self.friend_search_result = None;
        self.friend_search_gen = self.friend_search_gen.wrapping_add(1);
        if let Ok(mut status) = self.friend_add_status.lock() {
            status.clear();
        }
        if let Ok(mut pending) = self.friend_search_pending.lock() {
            *pending = None;
        }
    }

    /// Append a character to the add-friend query.
    pub fn friend_add_push(&mut self, c: char) {
        self.friend_add_query.push(c);
        // A fresh edit invalidates any previously resolved preview. Bump the
        // generation and drop any in-flight/published search result so a late
        // worker can't resurrect a stale preview (and add target) for the
        // now-edited query — its tagged result will no longer match.
        self.friend_search_result = None;
        self.friend_search_gen = self.friend_search_gen.wrapping_add(1);
        if let Ok(mut pending) = self.friend_search_pending.lock() {
            *pending = None;
        }
    }

    /// Delete the last character from the add-friend query.
    pub fn friend_add_pop(&mut self) {
        self.friend_add_query.pop();
        self.friend_search_result = None;
        self.friend_search_gen = self.friend_search_gen.wrapping_add(1);
        if let Ok(mut pending) = self.friend_search_pending.lock() {
            *pending = None;
        }
    }

    /// Resolve the typed reference to a concrete account via `friends search`,
    /// off the UI thread. The worker publishes the result into shared cells that
    /// [`Browser::poll_friends_ops`] adopts; the status line reflects progress.
    pub fn friend_search(&mut self) {
        let query = self.friend_add_query.trim().to_string();
        if query.is_empty() {
            return;
        }
        if let Ok(mut status) = self.friend_add_status.lock() {
            *status = "searching...".to_string();
        }
        let status = self.friend_add_status.clone();
        let pending = self.friend_search_pending.clone();
        // Tag the worker with the generation it was launched for; the poll only
        // adopts a result whose tag still matches the current generation.
        let search_gen = self.friend_search_gen;
        thread::spawn(move || match aurelia::friends_search(&query) {
            Ok(found) => {
                if let Ok(mut status) = status.lock() {
                    *status = format!("Found {}", found.display_name());
                }
                if let Ok(mut pending) = pending.lock() {
                    *pending = Some((search_gen, found));
                }
            }
            Err(err) => {
                if let Ok(mut status) = status.lock() {
                    *status = format!("Not found: {}", err);
                }
            }
        });
    }

    /// Send a friend request for the resolved account (or, if no search has run,
    /// the raw typed query) via `friends add`, off the UI thread. On success the
    /// roster is flagged for refresh and the overlay reports the outcome.
    pub fn friend_add_confirm(&mut self) {
        // Prefer the resolved SteamID; fall back to the raw query so `add` still
        // works if the user skipped the search step.
        let target = match &self.friend_search_result {
            Some(found) => found.steam_id.to_string(),
            None => self.friend_add_query.trim().to_string(),
        };
        if target.is_empty() {
            return;
        }
        if let Ok(mut status) = self.friend_add_status.lock() {
            *status = "sending request...".to_string();
        }
        let status = self.friend_add_status.clone();
        let refresh = self.friends_refresh_pending.clone();
        thread::spawn(move || match aurelia::friends_add(&target) {
            Ok(()) => {
                if let Ok(mut status) = status.lock() {
                    *status = "Request sent.".to_string();
                }
                if let Ok(mut refresh) = refresh.lock() {
                    *refresh = true;
                }
            }
            Err(err) => {
                if let Ok(mut status) = status.lock() {
                    *status = format!("Failed: {}", err);
                }
            }
        });
    }

    /// Remove the highlighted friend (or cancel a pending request) via
    /// `friends remove`, off the UI thread. On success the roster is flagged for
    /// refresh. A missing selection is a no-op.
    pub fn friend_remove_confirm(&mut self) {
        let Some(friend) = self.selected_friend() else {
            return;
        };
        let steamid = friend.steam_id;
        let refresh = self.friends_refresh_pending.clone();
        thread::spawn(move || {
            if aurelia::friends_remove(steamid).is_ok() {
                if let Ok(mut refresh) = refresh.lock() {
                    *refresh = true;
                }
            }
        });
    }

    /// Apply any async friend-management results: adopt a resolved search into
    /// the preview, and re-fetch the roster (off the UI thread) when an add or
    /// remove has succeeded. Called once per event-loop iteration.
    pub fn poll_friends_ops(&mut self) {
        // Adopt a resolved search result published by the worker thread, but only
        // if it was launched for the current query generation. A result tagged
        // with a stale generation (the query was edited or the overlay toggled
        // after the worker spawned) is dropped, closing the TOCTOU where a late
        // worker could briefly revive a preview for the now-edited query.
        let adopted = self
            .friend_search_pending
            .lock()
            .ok()
            .and_then(|mut pending| pending.take());
        if let Some((search_gen, found)) = adopted {
            if search_gen == self.friend_search_gen {
                self.friend_search_result = Some(found);
            }
        }

        // Re-fetch the roster after a successful add/remove. The fetch itself is
        // a quick CLI call; run it on a worker and adopt the list next poll via
        // the same `friends` field (kept simple: a one-shot blocking re-fetch on
        // a detached thread that swaps the vec in through a channel-free flag).
        let needs_refresh = self
            .friends_refresh_pending
            .lock()
            .map(|mut flag| std::mem::replace(&mut *flag, false))
            .unwrap_or(false);
        if needs_refresh {
            // Fetch off the UI thread, then publish into the search-pending-style
            // roster cell adopted on a later poll.
            let roster = self.friends_roster_pending.clone();
            thread::spawn(move || {
                let list = aurelia::friends().unwrap_or_default();
                if let Ok(mut slot) = roster.lock() {
                    *slot = Some(list);
                }
            });
        }

        // Adopt a freshly fetched roster, if one is ready, keeping the highlight
        // in range.
        if let Ok(mut slot) = self.friends_roster_pending.lock() {
            if let Some(list) = slot.take() {
                self.friends = list;
                self.friends_loaded = true;
                self.friends_loading = false;
                if self.friends_index >= self.friends.len() {
                    self.friends_index = self.friends.len().saturating_sub(1);
                }
            }
        }
    }

    /// Open a dedicated chat **in a new terminal window** with the highlighted
    /// friend: spawns this same binary with `--chat <steamid> <name>`, which
    /// launches straight into the full-screen chat loop. A missing selection or
    /// spawn failure is a no-op (the panel stays as-is).
    pub fn open_chat_terminal(&self) {
        let Some(friend) = self.selected_friend() else {
            return;
        };
        let steamid = friend.steam_id;
        let name = friend.display_name();
        let Ok(exe) = std::env::current_exe() else {
            return;
        };
        let exe = exe.to_string_lossy().into_owned();
        let id = steamid.to_string();

        #[cfg(windows)]
        {
            // `cmd /c start "<title>" "<program>" <args...>` opens a new console
            // window. The quoted title positional is required so `start` does not
            // treat the quoted program path as the window title.
            let _ = std::process::Command::new("cmd")
                .args(["/c", "start", "Aurelia Chat", &exe, "--chat", &id, &name])
                .spawn();
        }
        #[cfg(not(windows))]
        {
            // Best-effort: launch the chat binary directly, detached.
            let _ = std::process::Command::new(&exe)
                .args(["--chat", &id, &name])
                .spawn();
        }
    }

    // --- Chat view ---

    /// Open the chat view for the highlighted friend. Sets the partner, clears
    /// the input, and fetches recent history (a fetch error opens an empty
    /// conversation). No-op when no friend is selected.
    pub fn open_chat(&mut self) {
        let Some(friend) = self.selected_friend() else {
            return;
        };
        let steamid = friend.steam_id;
        let partner = friend.display_name();
        self.chat_steamid = steamid;
        self.chat_partner = partner;
        self.chat_input.clear();
        self.chat_scroll = 0;
        self.chat_messages = aurelia::chat_history(self.chat_steamid, 30).unwrap_or_default();
        self.show_chat = true;
    }

    /// Close the chat view and drop its state.
    pub fn close_chat(&mut self) {
        self.show_chat = false;
        self.chat_messages = Vec::new();
        self.chat_steamid = 0;
        self.chat_partner = String::new();
        self.chat_input.clear();
        self.chat_scroll = 0;
    }

    /// Re-fetch the open conversation's history (e.g. after sending a message).
    pub fn refresh_chat(&mut self) {
        self.chat_messages = aurelia::chat_history(self.chat_steamid, 30).unwrap_or_default();
        // The chat panel is bottom-anchored and renders the Vec top→bottom, so it
        // must be oldest→newest. The CLI returns history newest-first, so reverse
        // it; then a *stable* sort by send time enforces true chronological order
        // when timestamps are present, and is a harmless no-op (keeping the
        // reversed oldest→newest order) when they are not.
        self.chat_messages.reverse();
        self.chat_messages.sort_by_key(|m| m.timestamp);
    }

    /// Send the composed message to the chat partner (blocking), then clear the
    /// input and re-fetch the history. No-op when the input is empty.
    pub fn chat_send(&mut self) {
        if self.chat_input.is_empty() {
            return;
        }
        let _ = aurelia::chat_send(self.chat_steamid, &self.chat_input);
        self.chat_input.clear();
        self.refresh_chat();
    }

    /// Append a typed character to the composed message.
    pub fn chat_push(&mut self, c: char) {
        self.chat_input.push(c);
    }

    /// Remove the last character from the composed message.
    pub fn chat_pop(&mut self) {
        self.chat_input.pop();
    }

    /// Fetch the inventory for `app_id` (blocking) and open the overlay. A fetch
    /// error simply opens an empty overlay ("No inventory items.").
    pub fn open_inventory(&mut self, app_id: i32) {
        self.inventory = aurelia::inventory(app_id).unwrap_or_default();
        self.inv_scroll = 0;
        self.show_inventory = true;
    }

    /// Close the inventory overlay and drop its data.
    pub fn close_inventory(&mut self) {
        self.show_inventory = false;
        self.inventory = Vec::new();
        self.inv_scroll = 0;
    }

    /// Scroll the inventory overlay down by one row (clamped).
    pub fn inv_scroll_down(&mut self) {
        let max = self.inventory.len().saturating_sub(1);
        if self.inv_scroll < max {
            self.inv_scroll += 1;
        }
    }

    /// Scroll the inventory overlay up by one row (clamped).
    pub fn inv_scroll_up(&mut self) {
        self.inv_scroll = self.inv_scroll.saturating_sub(1);
    }

    // --- Launch-options overlay ---

    /// Fetch the launch options for `app_id` (blocking) and open the overlay. A
    /// fetch error simply opens an empty overlay ("No launch options.").
    pub fn open_launch(&mut self, app_id: i32) {
        self.launch_options = aurelia::launch_options(app_id).unwrap_or_default();
        self.launch_scroll = 0;
        self.show_launch = true;
    }

    /// Close the launch-options overlay and drop its data.
    pub fn close_launch(&mut self) {
        self.show_launch = false;
        self.launch_options = Vec::new();
        self.launch_scroll = 0;
    }

    /// Scroll the launch-options overlay down by one row (clamped).
    pub fn launch_scroll_down(&mut self) {
        let max = self.launch_options.len().saturating_sub(1);
        if self.launch_scroll < max {
            self.launch_scroll += 1;
        }
    }

    /// Scroll the launch-options overlay up by one row (clamped).
    pub fn launch_scroll_up(&mut self) {
        self.launch_scroll = self.launch_scroll.saturating_sub(1);
    }

    /// Fetch the account's market listings (blocking) and open the overlay. A
    /// fetch error simply opens an empty overlay ("No active listings.").
    pub fn open_market(&mut self) {
        self.market = aurelia::market_listings().unwrap_or_default();
        self.market_scroll = 0;
        self.show_market = true;
    }

    /// Close the market overlay and drop its data.
    pub fn close_market(&mut self) {
        self.show_market = false;
        self.market = Vec::new();
        self.market_scroll = 0;
    }

    /// Scroll the market overlay down by one row (clamped).
    pub fn market_scroll_down(&mut self) {
        let max = self.market.len().saturating_sub(1);
        if self.market_scroll < max {
            self.market_scroll += 1;
        }
    }

    /// Scroll the market overlay up by one row (clamped).
    pub fn market_scroll_up(&mut self) {
        self.market_scroll = self.market_scroll.saturating_sub(1);
    }

    // --- Community Market search overlay ---

    /// Open the Community Market search overlay, clearing any previous query and
    /// results. No backend call happens until the user submits a query.
    pub fn open_market_search(&mut self) {
        self.show_market_search = true;
        self.market_query.clear();
        self.market_results.clear();
        self.market_results_index = 0;
        self.market_results_scroll = 0;
        self.market_search_status.clear();
        // A fresh overlay invalidates any in-flight search; bump the generation so
        // a late worker's tagged result is dropped by `poll_market`.
        self.market_search_gen = self.market_search_gen.wrapping_add(1);
        if let Ok(mut slot) = self.market_search_cell.lock() {
            *slot = (self.market_search_gen, aurelia::MarketSearchState::Idle);
        }
        if let Ok(mut slot) = self.market_price_cell.lock() {
            *slot = aurelia::MarketPriceState::Idle;
        }
    }

    /// Close the market search overlay and drop its state.
    pub fn close_market_search(&mut self) {
        self.show_market_search = false;
        self.market_query.clear();
        self.market_results.clear();
        self.market_results_index = 0;
        self.market_results_scroll = 0;
        self.market_search_status.clear();
        // Invalidate any in-flight search so a late worker can't publish into the
        // next time the overlay is opened.
        self.market_search_gen = self.market_search_gen.wrapping_add(1);
    }

    /// Append a typed character to the search query.
    pub fn market_query_push(&mut self, c: char) {
        self.market_query.push(c);
        // A fresh edit invalidates any in-flight search for the prior query; bump
        // the generation so its tagged result is dropped by `poll_market`.
        self.market_search_gen = self.market_search_gen.wrapping_add(1);
    }

    /// Remove the last character from the search query.
    pub fn market_query_pop(&mut self) {
        self.market_query.pop();
        self.market_search_gen = self.market_search_gen.wrapping_add(1);
    }

    /// Kick off a Community Market search for the typed query, off the UI thread.
    /// The worker publishes into the shared cell; [`poll_market`] adopts it. A
    /// no-op when the query is blank.
    pub fn submit_market_search(&mut self) {
        let query = self.market_query.trim().to_string();
        if query.is_empty() {
            return;
        }
        self.market_search_status = "searching...".to_string();
        // Tag the worker with the generation it was launched for; `poll_market`
        // only adopts a result whose tag still matches the current generation, so
        // an out-of-order OLDER search can't clobber a NEWER one's results.
        let search_gen = self.market_search_gen;
        if let Ok(mut slot) = self.market_search_cell.lock() {
            *slot = (search_gen, aurelia::MarketSearchState::Loading);
        }
        let cell = Arc::clone(&self.market_search_cell);
        thread::spawn(move || aurelia::market_search_async(query, search_gen, cell));
    }

    /// Look up the highlighted result's market price, off the UI thread. The
    /// worker publishes into the shared price cell; [`poll_market`] adopts it. A
    /// no-op when no result is highlighted.
    pub fn lookup_market_price(&mut self) {
        let Some(result) = self.selected_market_result() else {
            return;
        };
        let app_id = result.app_id;
        let name = result.market_hash_name.clone();
        if name.is_empty() {
            self.market_search_status = "no item name to price".to_string();
            return;
        }
        self.market_search_status = format!("pricing {}...", result.display_name());
        if let Ok(mut slot) = self.market_price_cell.lock() {
            *slot = aurelia::MarketPriceState::Loading;
        }
        let cell = Arc::clone(&self.market_price_cell);
        thread::spawn(move || aurelia::market_price_async(app_id, name, cell));
    }

    /// Adopt any completed search/price result published by the worker threads,
    /// updating the results list and status line. Called once per frame from the
    /// event loop; cheap and non-blocking (no disk/CLI access on the UI thread).
    pub fn poll_market(&mut self) {
        if !self.show_market_search {
            return;
        }

        // Adopt a finished search, but only if it was launched for the current
        // query generation. A result tagged with a stale generation (the query
        // was edited or the overlay toggled after the worker spawned) is dropped,
        // closing the out-of-order clobber where a slow OLDER search could
        // overwrite a NEWER one's results.
        let search = self.market_search_cell.lock().ok().and_then(|mut slot| {
            let search_gen = slot.0;
            let done = !matches!(
                &slot.1,
                aurelia::MarketSearchState::Idle | aurelia::MarketSearchState::Loading
            );
            if done && search_gen == self.market_search_gen {
                let (_, state) = std::mem::replace(
                    &mut *slot,
                    (self.market_search_gen, aurelia::MarketSearchState::Idle),
                );
                Some(state)
            } else if done {
                // Stale result: discard it (reset to Idle) without adopting.
                slot.1 = aurelia::MarketSearchState::Idle;
                None
            } else {
                None
            }
        });
        match search {
            Some(aurelia::MarketSearchState::Ready(results)) => {
                let count = results.len();
                self.market_results = results;
                self.market_results_index = 0;
                self.market_results_scroll = 0;
                self.market_search_status = if count == 0 {
                    "no results".to_string()
                } else {
                    format!("{} results", count)
                };
            }
            Some(aurelia::MarketSearchState::Failed(err)) => {
                self.market_results.clear();
                self.market_results_index = 0;
                self.market_results_scroll = 0;
                self.market_search_status = format!("Failed: {}", err);
            }
            _ => {}
        }

        // Adopt a finished price lookup (does not disturb the results list).
        let price = self
            .market_price_cell
            .lock()
            .ok()
            .map(|mut slot| {
                let done = !matches!(
                    &*slot,
                    aurelia::MarketPriceState::Idle | aurelia::MarketPriceState::Loading
                );
                if done {
                    std::mem::replace(&mut *slot, aurelia::MarketPriceState::Idle)
                } else {
                    aurelia::MarketPriceState::Loading
                }
            });
        match price {
            Some(aurelia::MarketPriceState::Ready(p)) => {
                self.market_search_status = match p.summary() {
                    Some(s) => format!("{} — {}", p.market_hash_name, s),
                    None => format!("{} — no price data", p.market_hash_name),
                };
            }
            Some(aurelia::MarketPriceState::Failed(err)) => {
                self.market_search_status = format!("Price failed: {}", err);
            }
            _ => {}
        }
    }

    /// The highlighted search result, if any.
    pub fn selected_market_result(&self) -> Option<&aurelia::MarketSearchResultJson> {
        self.market_results.get(self.market_results_index)
    }

    /// Move the search-results highlight down by one row (clamped). The widget
    /// derives its scroll window from the index so the highlight stays visible.
    pub fn market_result_next(&mut self) {
        if self.market_results.is_empty() {
            self.market_results_index = 0;
            return;
        }
        if self.market_results_index + 1 < self.market_results.len() {
            self.market_results_index += 1;
        }
    }

    /// Move the search-results highlight up by one row (clamped).
    pub fn market_result_previous(&mut self) {
        self.market_results_index = self.market_results_index.saturating_sub(1);
    }

    /// Fetch the given game's subscribed Workshop items (blocking) and open the
    /// overlay in its subscribed-list mode. A fetch error simply opens an empty
    /// overlay ("No workshop items.").
    pub fn open_workshop(&mut self, app_id: i32) {
        self.workshop = aurelia::workshop_list(app_id).unwrap_or_default();
        self.workshop_scroll = 0;
        self.workshop_app_id = app_id;
        self.workshop_browse = false;
        self.workshop_query.clear();
        self.workshop_results = Vec::new();
        self.workshop_index = 0;
        self.workshop_searching = false;
        self.workshop_status.clear();
        self.workshop_rating = false;
        self.close_workshop_comments();
        self.show_workshop = true;
    }

    /// Close the Workshop overlay and drop its data.
    pub fn close_workshop(&mut self) {
        self.show_workshop = false;
        self.workshop = Vec::new();
        self.workshop_scroll = 0;
        self.workshop_browse = false;
        self.workshop_query.clear();
        self.workshop_results = Vec::new();
        self.workshop_index = 0;
        self.workshop_searching = false;
        self.workshop_status.clear();
        self.workshop_rating = false;
        self.close_workshop_comments();
        // Bump the generation so any in-flight worker's late result is dropped.
        self.workshop_gen = self.workshop_gen.wrapping_add(1);
    }

    /// Scroll the Workshop overlay down by one row (clamped).
    pub fn workshop_scroll_down(&mut self) {
        let max = self.workshop.len().saturating_sub(1);
        if self.workshop_scroll < max {
            self.workshop_scroll += 1;
        }
    }

    /// Scroll the Workshop overlay up by one row (clamped).
    pub fn workshop_scroll_up(&mut self) {
        self.workshop_scroll = self.workshop_scroll.saturating_sub(1);
    }

    /// Switch the Workshop overlay into browse/search mode and kick off an
    /// initial (default feed) browse for the current game.
    pub fn workshop_enter_browse(&mut self) {
        if self.workshop_browse {
            return;
        }
        self.workshop_browse = true;
        self.workshop_status.clear();
        self.workshop_index = 0;
        self.start_workshop_search();
    }

    /// Leave browse mode and return to the subscribed-items list.
    pub fn workshop_exit_browse(&mut self) {
        self.workshop_browse = false;
        self.workshop_status.clear();
        // Drop any in-flight search result.
        self.workshop_gen = self.workshop_gen.wrapping_add(1);
        self.workshop_searching = false;
        self.workshop_rating = false;
        self.close_workshop_comments();
    }

    /// Append a character to the browse query (browse mode only).
    pub fn workshop_push_query(&mut self, c: char) {
        self.workshop_query.push(c);
    }

    /// Delete the last character of the browse query (browse mode only).
    pub fn workshop_pop_query(&mut self) {
        self.workshop_query.pop();
    }

    /// Move the highlight down within the browse results (clamped).
    pub fn workshop_next(&mut self) {
        let max = self.workshop_results.len().saturating_sub(1);
        if self.workshop_index < max {
            self.workshop_index += 1;
        }
    }

    /// Move the highlight up within the browse results (clamped).
    pub fn workshop_previous(&mut self) {
        self.workshop_index = self.workshop_index.saturating_sub(1);
    }

    /// The currently highlighted browse result, if any.
    pub fn selected_workshop_result(&self) -> Option<&aurelia::WorkshopItemJson> {
        self.workshop_results.get(self.workshop_index)
    }

    /// Launch a `workshop browse` request off the UI thread for the current
    /// query. A monotonic generation tag is captured up front; when the worker
    /// finishes it posts `(generation, result)` into the shared slot, and
    /// `poll_workshop` only applies it if the tag still matches — so a slow
    /// earlier search can never overwrite a newer one.
    pub fn start_workshop_search(&mut self) {
        self.workshop_gen = self.workshop_gen.wrapping_add(1);
        let generation = self.workshop_gen;
        let app_id = self.workshop_app_id;
        let query = self.workshop_query.clone();
        let slot = Arc::clone(&self.workshop_slot);
        self.workshop_searching = true;
        self.workshop_status.clear();
        thread::spawn(move || {
            let result =
                aurelia::workshop_browse(app_id, &query).map_err(|e| e.to_string());
            if let Ok(mut guard) = slot.lock() {
                *guard = Some((generation, result));
            }
        });
    }

    /// Drain a finished browse worker's result into the overlay state. Called
    /// once per event-loop iteration; cheap and non-blocking when idle. Stale
    /// results (generation mismatch) are discarded.
    pub fn poll_workshop(&mut self) {
        let posted = match self.workshop_slot.lock() {
            Ok(mut guard) => guard.take(),
            Err(_) => None,
        };
        if let Some((generation, result)) = posted {
            if generation != self.workshop_gen {
                return; // a newer search superseded this one
            }
            self.workshop_searching = false;
            match result {
                Ok(items) => {
                    self.workshop_results = items;
                    self.workshop_index = 0;
                    if self.workshop_results.is_empty() {
                        self.workshop_status = "No matches.".to_string();
                    } else {
                        self.workshop_status.clear();
                    }
                }
                Err(err) => {
                    self.workshop_results = Vec::new();
                    self.workshop_index = 0;
                    self.workshop_status = format!("Browse failed: {err}");
                }
            }
        }

        // Drain a finished subscribe/unsubscribe worker.
        let action = match self.workshop_action_slot.lock() {
            Ok(mut guard) => guard.take(),
            Err(_) => None,
        };
        if let Some((item_id, want_subscribed, result)) = action {
            self.workshop_acting = false;
            match result {
                Ok(()) => {
                    // Flip the flag on whichever result row carries this id (the
                    // highlight may have moved while the request was in flight).
                    for item in self.workshop_results.iter_mut() {
                        if item.id == Some(item_id) {
                            item.subscribed = want_subscribed;
                        }
                    }
                    self.workshop_status = if want_subscribed {
                        "Subscribed.".to_string()
                    } else {
                        "Unsubscribed.".to_string()
                    };
                    // The refreshed subscribed list (if any) arrives separately
                    // via `workshop_refresh_slot`, drained below — no blocking
                    // CLI call here on the render thread.
                }
                Err(err) => {
                    self.workshop_status = if want_subscribed {
                        format!("Subscribe failed: {err}")
                    } else {
                        format!("Unsubscribe failed: {err}")
                    };
                }
            }
        }

        // Drain a re-fetched subscribed list posted by a successful action
        // worker. Non-blocking try-lock + take; install only if the generation
        // still matches so a refresh that arrives after the overlay closed or
        // moved on is discarded.
        let refresh = match self.workshop_refresh_slot.lock() {
            Ok(mut guard) => guard.take(),
            Err(_) => None,
        };
        if let Some((generation, items)) = refresh {
            if generation == self.workshop_gen {
                self.workshop = items;
                let max = self.workshop.len().saturating_sub(1);
                if self.workshop_scroll > max {
                    self.workshop_scroll = max;
                }
            }
        }

        // Drain a finished rate worker. The generation is checked so a late
        // result for a since-closed/superseded overlay is dropped silently.
        let rated = match self.workshop_rate_slot.lock() {
            Ok(mut guard) => guard.take(),
            Err(_) => None,
        };
        if let Some((generation, result)) = rated {
            // The in-flight rate is definitively finished, so always clear the
            // re-press guard — otherwise bumping the generation while a rate is
            // in flight (a new search, or closing the comments sub-pane) would
            // strand `workshop_rating = true` and wedge rating forever. Only the
            // user-visible status line is gated on the generation still matching.
            self.workshop_rating = false;
            if generation == self.workshop_gen {
                self.workshop_status = match result {
                    Ok(()) => "Rated.".to_string(),
                    Err(err) => format!("Rate failed: {err}"),
                };
            }
        }

        // Drain a finished comments fetch. Apply only when the generation still
        // matches (the sub-pane was not closed / superseded since the fetch
        // started) and the sub-pane is still open.
        let comments = match self.workshop_comments_slot.lock() {
            Ok(mut guard) => guard.take(),
            Err(_) => None,
        };
        if let Some((generation, result)) = comments {
            if generation == self.workshop_gen && self.workshop_comments_open {
                self.workshop_comments_loading = false;
                match result {
                    Ok(items) => {
                        self.workshop_comments = items;
                        self.workshop_comments_scroll = 0;
                        self.workshop_comments_status = if self.workshop_comments.is_empty() {
                            "No comments.".to_string()
                        } else {
                            String::new()
                        };
                    }
                    Err(err) => {
                        self.workshop_comments = Vec::new();
                        self.workshop_comments_scroll = 0;
                        self.workshop_comments_status = format!("Comments failed: {err}");
                    }
                }
            }
        }
    }

    /// Toggle the subscription of the highlighted browse result off the UI
    /// thread: subscribe if it is not currently subscribed, otherwise
    /// unsubscribe. The worker posts `(item_id, want_subscribed, result)` into
    /// `workshop_action_slot`, which `poll_workshop` drains to flip the local
    /// flag and refresh the subscribed list. A request already in flight is a
    /// no-op so a held key cannot stack calls.
    pub fn workshop_toggle_subscribe_selected(&mut self) {
        if self.workshop_acting {
            return;
        }
        let Some(item) = self.selected_workshop_result() else {
            return;
        };
        let Some(id) = item.id else {
            return;
        };
        let want_subscribed = !item.subscribed;
        let slot = Arc::clone(&self.workshop_action_slot);
        let refresh_slot = Arc::clone(&self.workshop_refresh_slot);
        let app_id = self.workshop_app_id;
        let generation = self.workshop_gen;
        self.workshop_acting = true;
        self.workshop_status = if want_subscribed {
            "Subscribing…".to_string()
        } else {
            "Unsubscribing…".to_string()
        };
        thread::spawn(move || {
            let result = if want_subscribed {
                aurelia::workshop_subscribe(id)
            } else {
                aurelia::workshop_unsubscribe(id)
            }
            .map_err(|e| e.to_string());
            // On success, re-fetch the subscribed list *here on the worker
            // thread* (another blocking shell-out) and hand it back tagged with
            // the generation captured up front, so the UI thread never blocks on
            // `workshop list` and a stale refresh is dropped on arrival.
            if result.is_ok() {
                if let Ok(items) = aurelia::workshop_list(app_id) {
                    if let Ok(mut guard) = refresh_slot.lock() {
                        *guard = Some((generation, items));
                    }
                }
            }
            if let Ok(mut guard) = slot.lock() {
                *guard = Some((id, want_subscribed, result));
            }
        });
    }

    /// Rate the highlighted browse result thumbs-up (`up = true`) or thumbs-down
    /// off the UI thread. The worker posts `(generation, result)` into
    /// `workshop_rate_slot`, which `poll_workshop` drains to update the status
    /// line. A request already in flight is a no-op so a held key cannot stack
    /// calls; a missing selection / id is a no-op.
    pub fn workshop_rate_selected(&mut self, up: bool) {
        if self.workshop_rating {
            return;
        }
        let Some(item) = self.selected_workshop_result() else {
            return;
        };
        let Some(id) = item.id else {
            return;
        };
        let slot = Arc::clone(&self.workshop_rate_slot);
        let generation = self.workshop_gen;
        self.workshop_rating = true;
        self.workshop_status = if up {
            "Rating up…".to_string()
        } else {
            "Rating down…".to_string()
        };
        thread::spawn(move || {
            let result = aurelia::workshop_rate(id, up).map_err(|e| e.to_string());
            if let Ok(mut guard) = slot.lock() {
                *guard = Some((generation, result));
            }
        });
    }

    /// Open the comments sub-pane for the highlighted browse result and kick off
    /// an off-thread fetch of its comments. The worker posts `(generation,
    /// result)` into `workshop_comments_slot`, drained by `poll_workshop`. A
    /// missing selection / id is a no-op.
    pub fn workshop_open_comments(&mut self) {
        let Some(item) = self.selected_workshop_result() else {
            return;
        };
        let Some(id) = item.id else {
            return;
        };
        self.workshop_comments_open = true;
        self.workshop_comments_loading = true;
        self.workshop_comments = Vec::new();
        self.workshop_comments_scroll = 0;
        self.workshop_comments_status.clear();
        self.workshop_comments_id = id;
        // Reuse the overlay generation so a fetch for a since-closed/superseded
        // sub-pane is dropped on arrival (the gen is bumped on close/exit).
        let generation = self.workshop_gen;
        let slot = Arc::clone(&self.workshop_comments_slot);
        thread::spawn(move || {
            let result = aurelia::workshop_comments(id, 30).map_err(|e| e.to_string());
            if let Ok(mut guard) = slot.lock() {
                *guard = Some((generation, result));
            }
        });
    }

    /// Close the comments sub-pane and drop its contents. Bumps the overlay
    /// generation so any in-flight comments fetch is discarded on arrival.
    pub fn close_workshop_comments(&mut self) {
        if self.workshop_comments_open {
            self.workshop_gen = self.workshop_gen.wrapping_add(1);
        }
        self.workshop_comments_open = false;
        self.workshop_comments_loading = false;
        self.workshop_comments = Vec::new();
        self.workshop_comments_scroll = 0;
        self.workshop_comments_status.clear();
        self.workshop_comments_id = 0;
    }

    /// The id of the item the comments sub-pane is showing.
    pub fn workshop_comments_id(&self) -> u64 {
        self.workshop_comments_id
    }

    /// Scroll the comments sub-pane down by one row (clamped).
    pub fn workshop_comments_scroll_down(&mut self) {
        let max = self.workshop_comments.len().saturating_sub(1);
        if self.workshop_comments_scroll < max {
            self.workshop_comments_scroll += 1;
        }
    }

    /// Scroll the comments sub-pane up by one row (clamped).
    pub fn workshop_comments_scroll_up(&mut self) {
        self.workshop_comments_scroll = self.workshop_comments_scroll.saturating_sub(1);
    }

    /// Fetch the logged-in account (`aurelia account`) and open the overlay.
    /// Blocking; returns the backend error if the call fails.
    pub fn open_account(&mut self) -> Result<(), STError> {
        self.account_info = Some(aurelia::account()?);
        self.show_account = true;
        Ok(())
    }

    /// Fetch the launcher configuration (`aurelia config show`) and open the
    /// config overlay. Blocking; returns the backend error if the call fails.
    pub fn open_config(&mut self) -> Result<(), STError> {
        self.config_info = Some(aurelia::config_show()?);
        // Best-effort: a missing/failed proxy read just leaves the row blank.
        self.config_proxy = aurelia::config_proxy_show().ok();
        self.show_config = true;
        Ok(())
    }

    /// Close the config overlay and drop its contents.
    pub fn close_config(&mut self) {
        self.show_config = false;
        self.config_info = None;
        self.config_proxy = None;
        self.config_proxy_input = None;
    }

    /// Re-fetch just the proxy setting (after an edit/clear), keeping the overlay
    /// open.
    fn refresh_proxy(&mut self) {
        self.config_proxy = aurelia::config_proxy_show().ok();
    }

    /// Start editing the proxy URL, prefilling the current value.
    pub fn begin_proxy_edit(&mut self) {
        let current = self
            .config_proxy
            .as_ref()
            .and_then(|p| p.url.clone())
            .unwrap_or_default();
        self.config_proxy_input = Some(current);
    }

    pub fn proxy_input_push(&mut self, c: char) {
        if let Some(s) = self.config_proxy_input.as_mut() {
            s.push(c);
        }
    }

    pub fn proxy_input_pop(&mut self) {
        if let Some(s) = self.config_proxy_input.as_mut() {
            s.pop();
        }
    }

    pub fn cancel_proxy_edit(&mut self) {
        self.config_proxy_input = None;
    }

    /// Commit the typed proxy URL (`aurelia config proxy <url>`); an empty value
    /// clears the proxy. A backend/validation error surfaces as a status notice.
    pub fn commit_proxy_edit(&mut self) {
        let Some(url) = self.config_proxy_input.take() else {
            return;
        };
        if let Err(e) = aurelia::config_proxy_set(url.trim()) {
            self.notice = Some(format!("Proxy: {e}"));
        }
        self.refresh_proxy();
    }

    /// Clear the configured proxy (`aurelia config proxy --clear`).
    pub fn clear_proxy(&mut self) {
        if let Err(e) = aurelia::config_proxy_clear() {
            self.notice = Some(format!("Proxy: {e}"));
        }
        self.refresh_proxy();
    }

    /// Fetch the Steam Wallet balance (`aurelia wallet`) and open the overlay.
    /// Blocking; returns the backend error if the call fails.
    pub fn open_wallet(&mut self) -> Result<(), STError> {
        self.wallet_info = Some(aurelia::wallet()?);
        self.show_wallet = true;
        Ok(())
    }

    /// Replace the library contents, keeping the current filter/query/sort and a
    /// sensible selection.
    pub fn set_items(&mut self, items: Vec<Game>) {
        self.items = items;
        self.clamp_selection();
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Whether a game passes the hidden/allowed-type rules and the active tab.
    fn passes(game: &Game, filter: Filter, config: &Config) -> bool {
        if config.hidden_games.contains(&game.id) {
            return false;
        }
        if !config.allowed_games.contains(&game.game_type) {
            return false;
        }
        match filter {
            Filter::All => true,
            Filter::Installed => game.installed,
            Filter::Updates => game.update_available,
            Filter::Favourites => config.favorite_games.contains(&game.id),
        }
    }

    /// The currently visible games: active tab + fuzzy query, sorted.
    pub fn visible(&self) -> Vec<&Game> {
        let config = Config::cached();
        let matcher = SkimMatcherV2::default();
        let mut games: Vec<&Game> = self
            .items
            .iter()
            .filter(|g| Browser::passes(g, self.filter, &config))
            .filter(|g| {
                self.query.is_empty()
                    || matcher.fuzzy_match(g.raw_name(), &self.query).is_some()
            })
            .collect();

        match self.sort {
            Sort::Name => {
                games.sort_by(|a, b| a.raw_name().to_lowercase().cmp(&b.raw_name().to_lowercase()));
            }
            Sort::Installed => {
                games.sort_by(|a, b| {
                    b.installed
                        .cmp(&a.installed)
                        .then_with(|| a.raw_name().to_lowercase().cmp(&b.raw_name().to_lowercase()))
                });
            }
        }
        games
    }

    pub fn selected(&self) -> Option<Game> {
        let visible = self.visible();
        self.state
            .selected()
            .and_then(|i| visible.get(i))
            .map(|g| (*g).clone())
    }

    pub fn selected_index(&self) -> Option<usize> {
        self.state.selected()
    }

    pub fn visible_len(&self) -> usize {
        self.visible().len()
    }

    /// Open the install-location picker for `app_id`, fetching the available
    /// Steam library folders (blocking). Returns `false` without opening if no
    /// libraries could be enumerated, so the caller can fall back to a default
    /// install.
    pub fn open_install_picker(&mut self, app_id: i32) -> bool {
        let libraries = aurelia::libraries().unwrap_or_default();
        if libraries.is_empty() {
            return false;
        }
        self.install_libraries = libraries;
        // The on-disk estimate, for showing the size and gauging fit. Treat 0
        // (unavailable) as unknown.
        self.install_estimate = aurelia::install_estimate(app_id).ok().filter(|&b| b > 0);
        self.install_picker_index = 0;
        self.show_install_picker = true;
        true
    }

    /// Close the install-location picker.
    pub fn close_install_picker(&mut self) {
        self.show_install_picker = false;
        self.install_libraries.clear();
        self.install_estimate = None;
        self.install_picker_index = 0;
    }

    /// Whether the highlighted library has room for the estimated install. True
    /// when either the estimate or the library's free space is unknown — don't
    /// block on missing data; the CLI re-checks authoritatively.
    pub fn selected_library_fits(&self) -> bool {
        match self.install_libraries.get(self.install_picker_index) {
            Some(lib) => match (self.install_estimate, lib.free_bytes) {
                (Some(est), Some(free)) => free >= est,
                _ => true,
            },
            None => false,
        }
    }

    /// Move the install-picker highlight down (clamped).
    pub fn install_picker_next(&mut self) {
        let max = self.install_libraries.len().saturating_sub(1);
        if self.install_picker_index < max {
            self.install_picker_index += 1;
        }
    }

    /// Move the install-picker highlight up (clamped).
    pub fn install_picker_previous(&mut self) {
        self.install_picker_index = self.install_picker_index.saturating_sub(1);
    }

    /// The library folder path currently highlighted in the install picker.
    pub fn selected_install_library(&self) -> Option<String> {
        self.install_libraries
            .get(self.install_picker_index)
            .map(|lib| lib.path.clone())
    }

    /// Register a fresh install control for `app_id` (replacing any prior one),
    /// remember the chosen `library` (so a later resume targets the same one),
    /// and return a clone to hand to [`crate::client::Client::install`]. Called
    /// when the UI starts (or resumes) an install.
    pub fn begin_install(
        &mut self,
        app_id: i32,
        library: Option<String>,
    ) -> aurelia::InstallControl {
        let control = aurelia::InstallControl::new();
        self.install_controls.insert(app_id, control.clone());
        self.install_library_choice.insert(app_id, library);
        control
    }

    /// The library a tracked install of `app_id` was started in, for resuming a
    /// paused download into the same place.
    pub fn install_library_for(&self, app_id: i32) -> Option<String> {
        self.install_library_choice
            .get(&app_id)
            .cloned()
            .flatten()
    }

    /// How the selected game's UI-tracked install currently sits. Reads the live
    /// status cell, but only for games this session actually started installing.
    pub fn install_phase(&self, game: &Game) -> InstallPhase {
        if !self.install_controls.contains_key(&game.id) {
            return InstallPhase::Idle;
        }
        let state = game.get_status().map(|s| s.state).unwrap_or_default();
        if state.starts_with("paused") {
            InstallPhase::Paused
        } else if state.contains("downloading")
            || state.contains("processing")
            || state.contains("queued")
            || state.contains("verifying")
            || state.contains("moving")
        {
            InstallPhase::Active
        } else {
            InstallPhase::Idle
        }
    }

    /// Pause the in-flight install of `app_id`: flag its control so the worker
    /// finalises as "paused", then ask the backend to stop the download (it is
    /// left on disk so [`Browser::begin_install`] resumes it). A no-op if no
    /// install is tracked for the game.
    pub fn pause_install(&mut self, app_id: i32) {
        if let Some(control) = self.install_controls.get(&app_id) {
            control.set(aurelia::InstallAction::Paused);
            let _ = aurelia::install_stop(app_id);
        }
    }

    /// Stop (cancel) the install of `app_id`: flag its control so the worker
    /// clears the status, then ask the backend to abort the download. The
    /// game is reset to not-installed immediately (badge + flag), and a
    /// background re-fetch confirms the on-disk state shortly after.
    pub fn stop_install(&mut self, app_id: i32) {
        if let Some(control) = self.install_controls.remove(&app_id) {
            self.install_library_choice.remove(&app_id);
            control.set(aurelia::InstallAction::Stopped);
            let _ = aurelia::install_stop(app_id);
            if let Some(game) = self.items.iter_mut().find(|g| g.id == app_id) {
                game.installed = false;
                if let Ok(mut s) = game.status_counter().lock() {
                    *s = None;
                }
            }
            self.request_game_refresh(app_id);
        }
    }

    /// Spawn an off-thread `list` fetch and drop the matching game's fresh entry
    /// into [`Browser::game_refresh_slot`] for the next poll to adopt. Used to
    /// reconcile one game's install state after an install completes/cancels
    /// without a full-screen reload.
    pub fn request_game_refresh(&self, app_id: i32) {
        let slot = Arc::clone(&self.game_refresh_slot);
        thread::spawn(move || {
            if let Ok(entries) = aurelia::fetch_library() {
                if let Some(entry) = entries.into_iter().find(|e| e.app_id as i32 == app_id) {
                    if let Ok(mut guard) = slot.lock() {
                        guard.push(entry);
                    }
                }
            }
        });
    }

    /// Adopt any freshly-fetched single-game entries: patch the in-memory game's
    /// install flags and drop its transient install status so the badge derives
    /// from the refreshed state. Cheap and non-blocking when idle.
    pub fn poll_game_refresh(&mut self) {
        let updates: Vec<aurelia::LibraryGameJson> = match self.game_refresh_slot.lock() {
            Ok(mut guard) => std::mem::take(&mut *guard),
            Err(_) => Vec::new(),
        };
        if updates.is_empty() {
            return;
        }
        for entry in updates {
            let id = entry.app_id as i32;
            if let Some(game) = self.items.iter_mut().find(|g| g.id == id) {
                game.installed = entry.is_installed;
                game.install_dir = entry.install_path.clone().unwrap_or_default();
                game.update_available = entry.update_available;
                if let Ok(mut s) = game.status_counter().lock() {
                    *s = None;
                }
            }
        }
        self.clamp_selection();
    }

    /// Detect tracked installs that have just finished and reconcile them. On
    /// success a targeted re-fetch updates the game's `installed` flag (so the
    /// listing no longer wrongly offers uninstall on a half-installed game); a
    /// failed install keeps its badge. Either way the finished control is
    /// dropped. Call once per event-loop iteration.
    pub fn poll_install_completions(&mut self) {
        let terminal: Vec<(i32, bool)> = self
            .install_controls
            .keys()
            .copied()
            .filter_map(|id| {
                let state = self
                    .items
                    .iter()
                    .find(|g| g.id == id)
                    .map(|g| g.get_status().map(|s| s.state).unwrap_or_default())
                    .unwrap_or_default();
                if state.starts_with("Installed!") || state == "done" {
                    Some((id, true))
                } else if state.starts_with("Failed") {
                    Some((id, false))
                } else {
                    None
                }
            })
            .collect();
        for (id, succeeded) in terminal {
            self.install_controls.remove(&id);
            self.install_library_choice.remove(&id);
            if succeeded {
                self.request_game_refresh(id);
            }
        }
    }

    /// Live tallies for the status bar, over the hidden/allowed-filtered universe.
    pub fn counts(&self) -> Counts {
        let config = Config::cached();
        let universe: Vec<&Game> = self
            .items
            .iter()
            .filter(|g| Browser::passes(g, Filter::All, &config))
            .collect();
        let installed = universe.iter().filter(|g| g.installed).count();
        let updates = universe.iter().filter(|g| g.update_available).count();
        let downloading = universe
            .iter()
            .filter(|g| {
                g.get_status()
                    .map(|s| {
                        let st = s.state;
                        st.contains("downloading") || st.contains("processing")
                    })
                    .unwrap_or(false)
            })
            .count();
        Counts {
            total: universe.len(),
            visible: self.visible_len(),
            installed,
            updates,
            downloading,
        }
    }

    // --- Selection movement ---

    fn reset_selection(&mut self) {
        let len = self.visible_len();
        self.state.select(if len == 0 { None } else { Some(0) });
    }

    /// Keep the selection within the (possibly shrunk) visible range.
    fn clamp_selection(&mut self) {
        let len = self.visible_len();
        match self.state.selected() {
            _ if len == 0 => self.state.select(None),
            Some(i) if i >= len => self.state.select(Some(len - 1)),
            None => self.state.select(Some(0)),
            _ => {}
        }
    }

    pub fn next(&mut self) {
        let len = self.visible_len();
        if len == 0 {
            self.state.select(None);
            return;
        }
        let i = match self.state.selected() {
            Some(i) if i + 1 < len => i + 1,
            Some(_) => 0,
            None => 0,
        };
        self.state.select(Some(i));
    }

    pub fn previous(&mut self) {
        let len = self.visible_len();
        if len == 0 {
            self.state.select(None);
            return;
        }
        let i = match self.state.selected() {
            Some(0) | None => len - 1,
            Some(i) => i - 1,
        };
        self.state.select(Some(i));
    }

    pub fn page_down(&mut self, page: usize) {
        let len = self.visible_len();
        if len == 0 {
            return;
        }
        let i = self.state.selected().unwrap_or(0);
        self.state.select(Some((i + page).min(len - 1)));
    }

    pub fn page_up(&mut self, page: usize) {
        let len = self.visible_len();
        if len == 0 {
            return;
        }
        let i = self.state.selected().unwrap_or(0);
        self.state.select(Some(i.saturating_sub(page)));
    }

    pub fn home(&mut self) {
        if self.visible_len() > 0 {
            self.state.select(Some(0));
        }
    }

    pub fn end(&mut self) {
        let len = self.visible_len();
        if len > 0 {
            self.state.select(Some(len - 1));
        }
    }

    /// Select a visible row by index (e.g. from a mouse click), if in range.
    pub fn select_index(&mut self, i: usize) {
        if i < self.visible_len() {
            self.state.select(Some(i));
        }
    }

    /// Re-clamp the selection after the visible set may have changed out of band
    /// (e.g. a game was hidden, or favourites toggled while on the Favourites tab).
    pub fn refresh(&mut self) {
        self.clamp_selection();
    }

    // --- Filter / sort / query mutations (all re-clamp the selection) ---

    pub fn set_filter(&mut self, filter: Filter) {
        // Selecting a library filter always returns to the Library view (so the
        // "All → Friends" transition reverses cleanly when a filter is picked).
        self.view = View::Library;
        self.filter = filter;
        self.reset_selection();
    }

    /// The active tab's index along the combined tab ring: `0..TABS.len()` for
    /// the library filters, and `TABS.len()` for the Friends tab. The tab bar
    /// renders from this so the highlight tracks the view.
    pub fn tab_index(&self) -> usize {
        match self.view {
            View::Library => self.filter.index(),
            View::Friends => Filter::TABS.len(),
        }
    }

    /// Cycle through the combined tab ring (the four library filters followed by
    /// the Friends tab). Stepping right off Favourites lands on Friends (which
    /// loads the roster and focuses the panel); stepping right off Friends wraps
    /// back to All. The library filters re-clamp the selection as before.
    pub fn cycle_filter(&mut self, forward: bool) {
        // Slots: 0..TABS.len() are filters, TABS.len() is the Friends tab.
        let slots = Filter::TABS.len() + 1;
        let idx = self.tab_index();
        let next = if forward {
            (idx + 1) % slots
        } else {
            (idx + slots - 1) % slots
        };
        if next == Filter::TABS.len() {
            self.enter_friends();
        } else {
            self.set_filter(Filter::TABS[next]);
        }
    }

    pub fn cycle_sort(&mut self) {
        self.sort = self.sort.next();
        self.clamp_selection();
    }

    pub fn push_query(&mut self, c: char) {
        self.query.push(c);
        self.reset_selection();
    }

    pub fn pop_query(&mut self) {
        self.query.pop();
        self.reset_selection();
    }

    pub fn clear_query(&mut self) {
        self.query.clear();
        self.reset_selection();
    }

    // --- Actions menu (per-game command palette) ---

    /// The full set of actions applicable to the currently selected game, before
    /// the live filter is applied. Rows are grouped by `category` in display
    /// order; only actions that make sense for the game's state are included
    /// (e.g. `Update`/`Uninstall` only when installed, `Install` only when not).
    pub fn action_candidates(&self) -> Vec<ActionRow> {
        let mut rows: Vec<ActionRow> = Vec::new();
        let Some(game) = self.selected() else {
            return rows;
        };
        let installed = game.installed;
        let installing = self.install_phase(&game) != InstallPhase::Idle;

        const PLAY: &str = "Play & Install";
        if installing {
            rows.push(ActionRow { action: Action::PauseResume, category: PLAY, label: "Pause / resume install", key: "Space" });
            rows.push(ActionRow { action: Action::CancelInstall, category: PLAY, label: "Cancel install", key: "c" });
        }
        if installed {
            rows.push(ActionRow { action: Action::Play, category: PLAY, label: "Play", key: "" });
            rows.push(ActionRow { action: Action::Update, category: PLAY, label: "Update", key: "U" });
            rows.push(ActionRow { action: Action::Verify, category: PLAY, label: "Verify files", key: "v" });
            rows.push(ActionRow { action: Action::Uninstall, category: PLAY, label: "Uninstall", key: "x" });
        } else if !installing {
            rows.push(ActionRow { action: Action::Install, category: PLAY, label: "Install", key: "d" });
        }

        const VERS: &str = "Versions";
        rows.push(ActionRow { action: Action::Versions, category: VERS, label: "Versions & pinning", key: "V" });
        rows.push(ActionRow { action: Action::Branches, category: VERS, label: "Beta branches", key: "b" });

        const RT: &str = "Runtimes & config";
        rows.push(ActionRow { action: Action::GameSettings, category: RT, label: "Game settings", key: "" });
        rows.push(ActionRow { action: Action::Proton, category: RT, label: "Proton runtimes", key: "P" });
        rows.push(ActionRow { action: Action::Engine, category: RT, label: "Runtime plugins (umu / lux)", key: "E" });
        rows.push(ActionRow { action: Action::LaunchOptions, category: RT, label: "Launch options", key: "L" });

        const CONTENT: &str = "Content";
        rows.push(ActionRow { action: Action::Dlc, category: CONTENT, label: "DLC", key: "D" });
        rows.push(ActionRow { action: Action::Workshop, category: CONTENT, label: "Workshop", key: "W" });
        rows.push(ActionRow { action: Action::Cloud, category: CONTENT, label: "Cloud saves", key: "C" });
        rows.push(ActionRow { action: Action::Achievements, category: CONTENT, label: "Achievements", key: "a" });
        rows.push(ActionRow { action: Action::Depots, category: CONTENT, label: "Depots", key: "o" });
        rows.push(ActionRow { action: Action::Inventory, category: CONTENT, label: "Inventory", key: "I" });

        const LIB: &str = "Library";
        if installed {
            rows.push(ActionRow { action: Action::Move, category: LIB, label: "Move install", key: "M" });
            rows.push(ActionRow { action: Action::Relink, category: LIB, label: "Relink install", key: "K" });
        }
        rows.push(ActionRow { action: Action::Import, category: LIB, label: "Import install", key: "N" });
        rows.push(ActionRow { action: Action::Collections, category: LIB, label: "Collections", key: "O" });
        rows.push(ActionRow { action: Action::Favourite, category: LIB, label: "Toggle favourite", key: "f" });
        rows.push(ActionRow { action: Action::Hide, category: LIB, label: "Hide game", key: "H" });

        rows
    }

    /// The Actions menu rows after applying the live `actions_filter` (a
    /// case-insensitive substring match on the label). This is what the menu
    /// widget renders and what the selection indexes into.
    pub fn filtered_actions(&self) -> Vec<ActionRow> {
        let needle = self.actions_filter.to_lowercase();
        self.action_candidates()
            .into_iter()
            .filter(|r| needle.is_empty() || r.label.to_lowercase().contains(&needle))
            .collect()
    }

    /// Open the Actions menu for the selected game.
    pub fn open_actions(&mut self) {
        if let Some(game) = self.selected() {
            self.actions_app_id = game.id;
            self.actions_index = 0;
            self.actions_filter.clear();
            self.show_actions = true;
        }
    }

    /// Close the Actions menu.
    pub fn close_actions(&mut self) {
        self.show_actions = false;
        self.actions_filter.clear();
        self.actions_index = 0;
    }

    pub fn actions_next(&mut self) {
        let n = self.filtered_actions().len();
        if n == 0 {
            self.actions_index = 0;
            return;
        }
        self.actions_index = (self.actions_index + 1) % n;
    }

    pub fn actions_previous(&mut self) {
        let n = self.filtered_actions().len();
        if n == 0 {
            self.actions_index = 0;
            return;
        }
        self.actions_index = if self.actions_index == 0 {
            n - 1
        } else {
            self.actions_index - 1
        };
    }

    /// The action currently highlighted in the (filtered) menu.
    pub fn selected_action(&self) -> Option<Action> {
        self.filtered_actions().get(self.actions_index).map(|r| r.action)
    }

    /// Append to the Actions menu filter (typing narrows the list).
    pub fn actions_filter_push(&mut self, c: char) {
        self.actions_filter.push(c);
        self.actions_index = 0;
    }

    /// Backspace the Actions menu filter.
    pub fn actions_filter_pop(&mut self) {
        self.actions_filter.pop();
        self.actions_index = 0;
    }

    // --- Versions & pinning overlay ---

    /// Fetch the game's per-depot current manifests and pin/install state and
    /// open the overlay. A fetch error simply opens an empty overlay.
    pub fn open_versions(&mut self, app_id: i32) {
        self.versions_manifests = aurelia::manifests(app_id, None).unwrap_or_default();
        self.versions_available = aurelia::available(app_id).ok();
        self.versions_index = 0;
        self.versions_app_id = app_id;
        self.versions_status.clear();
        self.versions_input = None;
        self.show_versions = true;
    }

    /// Close the versions overlay and drop its contents.
    pub fn close_versions(&mut self) {
        self.show_versions = false;
        self.versions_manifests.clear();
        self.versions_available = None;
        self.versions_index = 0;
        self.versions_input = None;
    }

    pub fn versions_app_id(&self) -> i32 {
        self.versions_app_id
    }

    pub fn selected_manifest(&self) -> Option<&aurelia::DepotManifestInfo> {
        self.versions_manifests.get(self.versions_index)
    }

    pub fn versions_next(&mut self) {
        if self.versions_manifests.is_empty() {
            self.versions_index = 0;
            return;
        }
        self.versions_index = (self.versions_index + 1) % self.versions_manifests.len();
    }

    pub fn versions_previous(&mut self) {
        if self.versions_manifests.is_empty() {
            self.versions_index = 0;
            return;
        }
        self.versions_index = if self.versions_index == 0 {
            self.versions_manifests.len() - 1
        } else {
            self.versions_index - 1
        };
    }

    fn refresh_available(&mut self) {
        self.versions_available = aurelia::available(self.versions_app_id).ok();
    }

    /// Whether the game is currently pinned (so its updates are held).
    pub fn is_pinned(&self) -> bool {
        self.versions_available.as_ref().map(|a| a.pinned).unwrap_or(false)
    }

    /// Pin the game at its currently-installed depot versions (`aurelia pin`).
    pub fn pin_current(&mut self) {
        self.versions_status = match aurelia::pin(self.versions_app_id) {
            Ok(p) => format!("Pinned {} depot(s) at the installed version.", p.manifests.len()),
            Err(e) => format!("Pin failed: {e}"),
        };
        self.refresh_available();
    }

    /// Clear the game's pin so it updates normally again (`aurelia unpin`).
    pub fn unpin_current(&mut self) {
        self.versions_status = match aurelia::unpin(self.versions_app_id) {
            Ok(true) => "Unpinned — updates re-enabled.".to_string(),
            Ok(false) => "Was not pinned.".to_string(),
            Err(e) => format!("Unpin failed: {e}"),
        };
        self.refresh_available();
    }

    /// Start prompting for a manifest id to downgrade the highlighted depot to.
    pub fn begin_downgrade_input(&mut self) {
        if self.selected_manifest().is_some() {
            self.versions_input = Some(String::new());
        }
    }

    pub fn versions_input_push(&mut self, c: char) {
        if c.is_ascii_digit() {
            if let Some(s) = self.versions_input.as_mut() {
                s.push(c);
            }
        }
    }

    pub fn versions_input_pop(&mut self) {
        if let Some(s) = self.versions_input.as_mut() {
            s.pop();
        }
    }

    pub fn cancel_downgrade_input(&mut self) {
        self.versions_input = None;
    }

    /// Kick off a downgrade of the highlighted depot to the typed manifest id on
    /// a worker thread. Progress streams into `versions_busy` (shown in the
    /// overlay); the download runs off the UI thread so the TUI stays responsive.
    pub fn start_downgrade(&mut self) {
        let Some(depot) = self.selected_manifest().map(|m| m.depot_id) else {
            return;
        };
        let Some(text) = self.versions_input.take() else {
            return;
        };
        let Ok(manifest) = text.trim().parse::<u64>() else {
            self.versions_status = "Invalid manifest id.".to_string();
            return;
        };
        let app_id = self.versions_app_id;
        let busy = Arc::clone(&self.versions_busy);
        self.versions_status = format!("Downgrading depot {depot} → manifest {manifest}…");
        thread::spawn(move || {
            aurelia::downgrade(app_id, &[(depot, manifest)], None, false, false, busy);
        });
    }

    /// The current downgrade worker status, if one is in flight (or just finished).
    pub fn downgrade_busy_text(&self) -> Option<String> {
        self.versions_busy
            .lock()
            .unwrap()
            .as_ref()
            .map(|s| s.state.clone())
    }

    // --- Game settings overlay (per-game config overrides + launch script) ---

    /// Number of editable rows in the game-settings overlay: Runner, Forced
    /// Proton, Platform, Launch script.
    pub const GAME_CONFIG_ROWS: usize = 4;

    /// Fetch the game's per-game config, launch-script state, and the installed
    /// Proton list (for cycling the forced runtime) and open the overlay.
    pub fn open_game_config(&mut self, app_id: i32) {
        self.game_config = aurelia::config_game_show(app_id).ok();
        self.game_script = aurelia::scripts_show(app_id).ok();
        self.game_config_protons = aurelia::proton_list()
            .map(|v| v.into_iter().filter(|p| p.installed).map(|p| p.name).collect())
            .unwrap_or_default();
        self.game_config_app_id = app_id;
        self.game_config_index = 0;
        self.game_config_status.clear();
        self.show_game_config = true;
    }

    pub fn close_game_config(&mut self) {
        self.show_game_config = false;
        self.game_config = None;
        self.game_script = None;
        self.game_config_protons.clear();
        self.game_config_index = 0;
    }

    pub fn game_config_next(&mut self) {
        self.game_config_index = (self.game_config_index + 1) % Self::GAME_CONFIG_ROWS;
    }

    pub fn game_config_previous(&mut self) {
        self.game_config_index = if self.game_config_index == 0 {
            Self::GAME_CONFIG_ROWS - 1
        } else {
            self.game_config_index - 1
        };
    }

    fn refresh_game_config(&mut self) {
        let id = self.game_config_app_id;
        self.game_config = aurelia::config_game_show(id).ok();
        self.game_script = aurelia::scripts_show(id).ok();
    }

    /// Activate (Enter) the highlighted settings row, cycling its value.
    pub fn game_config_activate(&mut self) {
        let id = self.game_config_app_id;
        match self.game_config_index {
            // Runner: auto → umu → luxtorpeda → auto.
            0 => {
                let cur = self
                    .game_config
                    .as_ref()
                    .map(|c| c.runner.as_str())
                    .unwrap_or("auto");
                let res = match cur {
                    "umu" => {
                        let _ = aurelia::config_game_set_umu(id, false);
                        aurelia::config_game_set_native_engine(id, true)
                    }
                    "luxtorpeda" => aurelia::config_game_set_native_engine(id, false),
                    _ => aurelia::config_game_set_umu(id, true),
                };
                self.game_config_status = match res {
                    Ok(()) => "Runner updated.".to_string(),
                    Err(e) => format!("Failed: {e}"),
                };
            }
            // Forced Proton: cycle installed runtimes, then wrap to cleared.
            1 => {
                let cur = self
                    .game_config
                    .as_ref()
                    .and_then(|c| c.forced_proton_version.clone());
                let res = if self.game_config_protons.is_empty() {
                    aurelia::config_game_clear_proton(id)
                } else {
                    let next = match cur.as_deref() {
                        None => Some(0),
                        Some(v) => match self.game_config_protons.iter().position(|p| p == v) {
                            Some(i) if i + 1 < self.game_config_protons.len() => Some(i + 1),
                            _ => None,
                        },
                    };
                    match next {
                        Some(i) => aurelia::config_game_set_proton(id, &self.game_config_protons[i]),
                        None => aurelia::config_game_clear_proton(id),
                    }
                };
                self.game_config_status = match res {
                    Ok(()) => "Forced Proton updated.".to_string(),
                    Err(e) => format!("Failed: {e}"),
                };
            }
            // Platform: toggle windows ↔ linux.
            2 => {
                let cur = self
                    .game_config
                    .as_ref()
                    .and_then(|c| c.platform_preference.clone());
                let next = match cur.as_deref() {
                    Some("windows") => "linux",
                    _ => "windows",
                };
                self.game_config_status = match aurelia::config_game_set_platform(id, next) {
                    Ok(()) => format!("Platform set to {next}."),
                    Err(e) => format!("Failed: {e}"),
                };
            }
            // Launch script: create one if none, else clear the per-game override.
            _ => {
                let has_script = self.game_script.as_ref().map(|s| s.exists).unwrap_or(false);
                let res = if has_script {
                    aurelia::config_game_clear_launch_script(id)
                } else {
                    aurelia::scripts_new(id, false).map(|_| ())
                };
                self.game_config_status = match res {
                    Ok(()) if has_script => "Launch-script override cleared.".to_string(),
                    Ok(()) => "Launch script created.".to_string(),
                    Err(e) => format!("Failed: {e}"),
                };
            }
        }
        self.refresh_game_config();
    }

    /// Delete the game's launch-script file (`aurelia scripts remove`).
    pub fn game_config_remove_script(&mut self) {
        let id = self.game_config_app_id;
        self.game_config_status = match aurelia::scripts_remove(id) {
            Ok(()) => "Launch script removed.".to_string(),
            Err(e) => format!("Failed: {e}"),
        };
        self.refresh_game_config();
    }

    // --- Collections overlay ---

    /// Fetch the account's collections and open the overlay for `app_id` (the
    /// game that add/remove will act on).
    pub fn open_collections(&mut self, app_id: i32) {
        self.collections = aurelia::collections_list().unwrap_or_default();
        self.collections_app_id = app_id;
        self.collections_index = 0;
        self.collections_status.clear();
        self.collections_input = None;
        self.show_collections = true;
    }

    pub fn close_collections(&mut self) {
        self.show_collections = false;
        self.collections.clear();
        self.collections_index = 0;
        self.collections_input = None;
    }

    pub fn selected_collection(&self) -> Option<&aurelia::CollectionJson> {
        self.collections.get(self.collections_index)
    }

    pub fn collections_next(&mut self) {
        if self.collections.is_empty() {
            self.collections_index = 0;
            return;
        }
        self.collections_index = (self.collections_index + 1) % self.collections.len();
    }

    pub fn collections_previous(&mut self) {
        if self.collections.is_empty() {
            self.collections_index = 0;
            return;
        }
        self.collections_index = if self.collections_index == 0 {
            self.collections.len() - 1
        } else {
            self.collections_index - 1
        };
    }

    fn refresh_collections(&mut self) {
        self.collections = aurelia::collections_list().unwrap_or_default();
        if self.collections_index >= self.collections.len() {
            self.collections_index = self.collections.len().saturating_sub(1);
        }
    }

    pub fn add_game_to_selected_collection(&mut self) {
        let app = self.collections_app_id;
        let Some(name) = self.selected_collection().map(|c| c.name.clone()) else {
            return;
        };
        self.collections_status = match aurelia::collection_add(&name, &[app]) {
            Ok(()) => format!("Added to \"{name}\"."),
            Err(e) => format!("Failed: {e}"),
        };
        self.refresh_collections();
    }

    pub fn remove_game_from_selected_collection(&mut self) {
        let app = self.collections_app_id;
        let Some(name) = self.selected_collection().map(|c| c.name.clone()) else {
            return;
        };
        self.collections_status = match aurelia::collection_remove(&name, &[app]) {
            Ok(()) => format!("Removed from \"{name}\"."),
            Err(e) => format!("Failed: {e}"),
        };
        self.refresh_collections();
    }

    pub fn delete_selected_collection(&mut self) {
        let Some(name) = self.selected_collection().map(|c| c.name.clone()) else {
            return;
        };
        self.collections_status = match aurelia::collection_delete(&name) {
            Ok(()) => format!("Deleted \"{name}\"."),
            Err(e) => format!("Failed: {e}"),
        };
        self.refresh_collections();
    }

    pub fn collections_pull(&mut self) {
        self.collections_status = match aurelia::collections_pull() {
            Ok(()) => "Pulled collections from Steam Cloud.".to_string(),
            Err(e) => format!("Failed: {e}"),
        };
        self.refresh_collections();
    }

    pub fn collections_push(&mut self) {
        self.collections_status = match aurelia::collection_push() {
            Ok(()) => "Pushed collections to Steam Cloud.".to_string(),
            Err(e) => format!("Failed: {e}"),
        };
    }

    pub fn collections_sync(&mut self) {
        self.collections_status = match aurelia::collection_sync() {
            Ok(()) => "Synced collections with Steam Cloud.".to_string(),
            Err(e) => format!("Failed: {e}"),
        };
        self.refresh_collections();
    }

    /// Start the inline "new collection" prompt.
    pub fn begin_collection_create(&mut self) {
        self.collections_input = Some(String::new());
    }

    pub fn collections_input_push(&mut self, c: char) {
        if let Some(s) = self.collections_input.as_mut() {
            s.push(c);
        }
    }

    pub fn collections_input_pop(&mut self) {
        if let Some(s) = self.collections_input.as_mut() {
            s.pop();
        }
    }

    pub fn cancel_collection_create(&mut self) {
        self.collections_input = None;
    }

    pub fn commit_collection_create(&mut self) {
        let Some(name) = self.collections_input.take() else {
            return;
        };
        let name = name.trim().to_string();
        if name.is_empty() {
            return;
        }
        self.collections_status = match aurelia::collection_create(&name) {
            Ok(()) => format!("Created \"{name}\"."),
            Err(e) => format!("Failed: {e}"),
        };
        self.refresh_collections();
    }

    // --- Runtime plugins overlay (umu / luxtorpeda / steam-runtime) ---

    /// Number of plugin rows: umu, luxtorpeda, steam-runtime.
    pub const ENGINE_ROWS: usize = 3;

    /// Fetch the status of all three runtime plugins and open the overlay.
    pub fn open_engine(&mut self) {
        self.engine_umu = aurelia::umu_status().ok();
        self.engine_lux = aurelia::lux_status().ok();
        self.engine_steam_runtime = aurelia::steam_runtime_status().ok();
        self.engine_index = 0;
        self.engine_status.clear();
        self.show_engine = true;
    }

    pub fn close_engine(&mut self) {
        self.show_engine = false;
        self.engine_umu = None;
        self.engine_lux = None;
        self.engine_steam_runtime = None;
        self.engine_index = 0;
    }

    pub fn engine_next(&mut self) {
        self.engine_index = (self.engine_index + 1) % Self::ENGINE_ROWS;
    }

    pub fn engine_previous(&mut self) {
        self.engine_index = if self.engine_index == 0 {
            Self::ENGINE_ROWS - 1
        } else {
            self.engine_index - 1
        };
    }

    fn refresh_engine(&mut self) {
        self.engine_umu = aurelia::umu_status().ok();
        self.engine_lux = aurelia::lux_status().ok();
        self.engine_steam_runtime = aurelia::steam_runtime_status().ok();
    }

    /// Run a plugin action on the highlighted row. `key` is the in-overlay key
    /// pressed: e enable, d disable, i install, U update, x uninstall (umu/lux);
    /// i install, r repair (steam-runtime). Unsupported combos are ignored.
    ///
    /// Enable/disable/uninstall are instant config/fs writes, done synchronously.
    /// Install/update/repair download payloads over the network, so they run on a
    /// worker thread (streaming into `engine_busy`, adopted by [`poll_engine`]) —
    /// the overlay shows `working…` and the TUI stays responsive. A second action
    /// is ignored while one is in flight.
    pub fn engine_action(&mut self, key: char) {
        if self.engine_busy.lock().unwrap().is_some() {
            return;
        }

        // Instant, synchronous verbs.
        let sync_res: Option<Result<(), STError>> = match (self.engine_index, key) {
            (0, 'e') => Some(aurelia::umu_enable()),
            (0, 'd') => Some(aurelia::umu_disable()),
            (0, 'x') => Some(aurelia::umu_uninstall()),
            (1, 'e') => Some(aurelia::lux_enable()),
            (1, 'd') => Some(aurelia::lux_disable()),
            (1, 'x') => Some(aurelia::lux_uninstall()),
            _ => None,
        };
        if let Some(r) = sync_res {
            self.engine_status = match r {
                Ok(()) => "Done.".to_string(),
                Err(e) => format!("Failed: {e}"),
            };
            self.refresh_engine();
            return;
        }

        // Long, network-bound verbs: run off the UI thread.
        type Op = fn() -> Result<(), STError>;
        let op: Option<Op> = match (self.engine_index, key) {
            (0, 'i') => Some(aurelia::umu_install),
            (0, 'U') => Some(aurelia::umu_update),
            (1, 'i') => Some(aurelia::lux_install),
            (1, 'U') => Some(aurelia::lux_update),
            (2, 'i') => Some(aurelia::steam_runtime_install),
            (2, 'r') => Some(aurelia::steam_runtime_repair),
            _ => None,
        };
        if let Some(op) = op {
            let busy = Arc::clone(&self.engine_busy);
            *busy.lock().unwrap() = Some(GameStatus {
                state: "working…".to_string(),
                installdir: String::new(),
                size: 0.0,
            });
            self.engine_status.clear();
            thread::spawn(move || {
                let state = match op() {
                    Ok(()) => "Done.".to_string(),
                    Err(e) => format!("Failed: {e}"),
                };
                *busy.lock().unwrap() = Some(GameStatus {
                    state,
                    installdir: String::new(),
                    size: 0.0,
                });
            });
        }
    }

    /// The in-flight plugin-worker status (`working…`), if one is running.
    pub fn engine_busy_text(&self) -> Option<String> {
        self.engine_busy
            .lock()
            .unwrap()
            .as_ref()
            .filter(|s| s.state == "working…")
            .map(|s| s.state.clone())
    }

    /// Adopt a finished plugin worker's result: once `engine_busy` holds a
    /// terminal status (not `working…`), move it to `engine_status`, clear the
    /// cell, and re-fetch the plugin statuses. Called once per event loop.
    pub fn poll_engine(&mut self) {
        if !self.show_engine {
            return;
        }
        let terminal = {
            let guard = self.engine_busy.lock().unwrap();
            match guard.as_ref() {
                Some(s) if s.state != "working…" => Some(s.state.clone()),
                _ => None,
            }
        };
        if let Some(state) = terminal {
            self.engine_status = state;
            *self.engine_busy.lock().unwrap() = None;
            self.refresh_engine();
        }
    }
}

#[cfg(test)]
mod install_tests {
    use super::*;
    use crate::interface::aurelia::{InstallAction, InstallControl, LibraryGameJson};
    use crate::interface::game::Game;
    use crate::interface::game_status::GameStatus;

    fn game(id: i32) -> Game {
        Game::from_library(LibraryGameJson {
            app_id: id as u32,
            name: "Game".to_string(),
            is_installed: false,
            install_path: None,
            update_available: false,
            is_owned: true,
            is_family_shared: false,
            platform: None,
            active_branch: None,
            assets: None,
            store_url: None,
        })
    }

    fn set_state(g: &Game, state: &str) {
        *g.status_counter().lock().unwrap() = Some(GameStatus {
            state: state.to_string(),
            installdir: String::new(),
            size: 0.0,
        });
    }

    #[test]
    fn install_phase_only_tracks_started_installs() {
        let mut b = Browser::new(vec![]);
        let g = game(42);

        // An active-looking status on a game we never started installing is Idle.
        set_state(&g, "downloading 12.0%");
        assert_eq!(b.install_phase(&g), InstallPhase::Idle);

        // Once we begin an install, the live status drives the phase.
        b.begin_install(42, None);
        assert_eq!(b.install_phase(&g), InstallPhase::Active);

        set_state(&g, "paused 12.0%");
        assert_eq!(b.install_phase(&g), InstallPhase::Paused);

        // A finished install reads as Idle (so Space/c no longer act).
        set_state(&g, "Installed!");
        assert_eq!(b.install_phase(&g), InstallPhase::Idle);
    }

    #[test]
    fn poll_game_refresh_patches_install_flags_and_clears_status() {
        let g = game(99); // helper builds it not-installed
        let mut b = Browser::new(vec![g]);
        // A leftover transient status from the finished install.
        set_state(b.items.iter().find(|x| x.id == 99).unwrap(), "Installed!");

        // Simulate the off-thread worker dropping a fresh "now installed" entry.
        b.game_refresh_slot.lock().unwrap().push(LibraryGameJson {
            app_id: 99,
            name: "Game".to_string(),
            is_installed: true,
            install_path: Some("dir".to_string()),
            update_available: false,
            is_owned: true,
            is_family_shared: false,
            platform: None,
            active_branch: None,
            assets: None,
            store_url: None,
        });

        b.poll_game_refresh();

        let updated = b.items.iter().find(|x| x.id == 99).unwrap();
        assert!(updated.installed, "installed flag adopted from refreshed entry");
        // The transient status was dropped, so the badge now derives from the
        // installed flag (a steady "installed" state, not "Installed!").
        assert_eq!(updated.get_status().map(|s| s.state).as_deref(), Some("installed"));
    }

    #[test]
    fn install_control_action_roundtrips() {
        let c = InstallControl::new();
        assert_eq!(c.get(), InstallAction::Running);
        c.set(InstallAction::Paused);
        assert_eq!(c.get(), InstallAction::Paused);
        c.set(InstallAction::Stopped);
        assert_eq!(c.get(), InstallAction::Stopped);
    }
}
