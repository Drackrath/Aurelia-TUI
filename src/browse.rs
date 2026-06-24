//! Library browsing model: the single source of truth for what the user is
//! looking at. Combines a tab filter (All/Installed/Updates/Favourites), a live
//! fuzzy text query, and a sort order, and owns the selection. Every browse
//! widget renders from this; the event loop only mutates it.

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

    if state.contains("downloading") || state.contains("processing") || state.contains("verifying") {
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
    /// Whether the always-visible Friends panel currently holds keyboard focus
    /// (so j/k move the highlight and c/Enter/t act on the selected friend).
    pub friends_focused: bool,
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
            show_cloud: false,
            cloud_files: Vec::new(),
            cloud_status: String::new(),
            show_account: false,
            account_info: None,
            show_config: false,
            config_info: None,
            show_wallet: false,
            wallet_info: None,
            expand_description: false,
            show_achievements: false,
            achievements: Vec::new(),
            ach_scroll: 0,
            friends_focused: false,
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

    /// Toggle keyboard focus on the always-visible Friends panel. The first time
    /// the panel is focused its friends are fetched (blocking); a fetch error
    /// simply leaves the list empty ("Press [F] to load friends."). Unfocusing
    /// just drops focus — the list stays so it keeps showing in the panel.
    pub fn toggle_friends_focus(&mut self) {
        if self.friends_focused {
            self.friends_focused = false;
            return;
        }
        if self.friends.is_empty() {
            self.friends = aurelia::friends().unwrap_or_default();
            self.friends_index = 0;
        }
        self.friends_focused = true;
    }

    /// Drop focus on the Friends panel (Esc), keeping the loaded list.
    pub fn unfocus_friends(&mut self) {
        self.friends_focused = false;
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
        self.show_config = true;
        Ok(())
    }

    /// Close the config overlay and drop its contents.
    pub fn close_config(&mut self) {
        self.show_config = false;
        self.config_info = None;
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
        self.filter = filter;
        self.reset_selection();
    }

    pub fn cycle_filter(&mut self, forward: bool) {
        let idx = self.filter.index();
        let n = Filter::TABS.len();
        let next = if forward {
            (idx + 1) % n
        } else {
            (idx + n - 1) % n
        };
        self.set_filter(Filter::TABS[next]);
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
}
