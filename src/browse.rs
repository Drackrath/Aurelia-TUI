//! Library browsing model: the single source of truth for what the user is
//! looking at. Combines a tab filter (All/Installed/Updates/Favourites), a live
//! fuzzy text query, and a sort order, and owns the selection. Every browse
//! widget renders from this; the event loop only mutates it.

use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use tui::style::Style;
use tui::widgets::ListState;

use crate::config::Config;
use crate::interface::aurelia::{self, AccountJson};
use crate::interface::game::Game;
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
    /// Whether the description panel is expanded beyond its collapsed cap.
    pub expand_description: bool,
    /// Whether the achievements overlay is open.
    pub show_achievements: bool,
    /// The selected game's achievements (loaded when the overlay opens).
    pub achievements: Vec<aurelia::AchievementJson>,
    /// Scroll offset (top row) within the achievements overlay.
    pub ach_scroll: usize,
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
    /// Whether the depots overlay is open.
    pub show_depots: bool,
    /// Depots for the game the overlay was opened for.
    pub depots: Vec<aurelia::DepotJson>,
    /// Scroll offset (top row) within the depots overlay.
    pub depots_scroll: usize,
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
            expand_description: false,
            show_achievements: false,
            achievements: Vec::new(),
            ach_scroll: 0,
            confirm_uninstall: false,
            show_dlc: false,
            dlc: Vec::new(),
            dlc_index: 0,
            dlc_app_id: 0,
            show_depots: false,
            depots: Vec::new(),
            depots_scroll: 0,
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

    /// Sync the game's Steam Cloud saves (blocking), then re-fetch the list.
    pub fn sync_cloud(&mut self, app_id: i32) {
        self.cloud_status = "syncing...".to_string();
        if let Err(err) = aurelia::cloud_sync(app_id) {
            self.cloud_status = format!("Failed: {}", err);
            return;
        }
        self.refresh_cloud(app_id);
        if self.cloud_status.is_empty() {
            self.cloud_status = "synced".to_string();
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

    /// Fetch the logged-in account (`aurelia account`) and open the overlay.
    /// Blocking; returns the backend error if the call fails.
    pub fn open_account(&mut self) -> Result<(), STError> {
        self.account_info = Some(aurelia::account()?);
        self.show_account = true;
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
