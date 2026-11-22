//! Install queue overlay and download limit.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use super::Browser;
use crate::interface::game::Game;
use crate::interface::game_status::GameStatus;
use crate::interface::install_queue::{self as iq, ControlOutcome, ErrorKind, QueueEntry};

/// How often to re-read the queue.
const REFRESH: Duration = Duration::from_secs(2);

type Slot = Arc<Mutex<Option<Result<Vec<QueueEntry>, String>>>>;

/// Queue overlay state.
#[derive(Default)]
pub struct InstallQueue {
    pub show: bool,
    pub entries: Vec<QueueEntry>,
    pub index: usize,
    /// Footer line for results.
    pub status: String,
    /// Pending delete-files confirmation.
    pub confirm_delete: Option<u32>,
    /// Whether one list arrived.
    pub loaded: bool,
    slot: Slot,
    fetching: bool,
    last_fetch: Option<Instant>,
    /// Daemon installs without a local stream.
    adopted: HashSet<i32>,
    /// Startup adoption ran.
    scanned: bool,
}

impl InstallQueue {
    /// Re-adopt after a library reload.
    pub fn rescan(&mut self) {
        self.scanned = false;
        self.adopted.clear();
        self.last_fetch = None;
    }

    pub fn selected(&self) -> Option<&QueueEntry> {
        self.entries.get(self.index)
    }
}

/// Status text for a backlog entry.
pub fn entry_status(entry: &QueueEntry) -> Option<String> {
    let pct = format!("{:.1}%", entry.progress_percent());
    if entry.is_running() {
        return Some(format!("downloading {pct}"));
    }
    if !entry.is_resumable() {
        return None;
    }
    let mut suffix: Vec<String> = Vec::new();
    if entry.state != "paused" {
        suffix.push(entry.state.clone());
    }
    if let Some(kind) = entry.error_type.as_deref().map(ErrorKind::parse)
        && kind != ErrorKind::Unknown
    {
        suffix.push(kind.title().to_string());
    }
    Some(iq::paused_text(&pct, Some(&suffix.join(" · "))))
}

fn set_cell(game: &Game, text: Option<String>) {
    if let Ok(mut guard) = game.status_counter().lock() {
        *guard = text.map(|t| GameStatus::msg(&guard, &t));
    }
}

fn cell_is_live(game: &Game) -> bool {
    game.status_counter()
        .lock()
        .map(|g| g.is_some())
        .unwrap_or(true)
}

impl Browser {
    /// Open the queue and fetch it.
    pub fn open_install_queue(&mut self) {
        self.install_queue.show = true;
        self.install_queue.confirm_delete = None;
        self.install_queue.last_fetch = None;
    }

    pub fn close_install_queue(&mut self) {
        self.install_queue.show = false;
        self.install_queue.confirm_delete = None;
        self.install_queue.status.clear();
    }

    pub fn install_queue_next(&mut self) {
        let q = &mut self.install_queue;
        if q.index + 1 < q.entries.len() {
            q.index += 1;
        }
        q.confirm_delete = None;
    }

    pub fn install_queue_previous(&mut self) {
        let q = &mut self.install_queue;
        q.index = q.index.saturating_sub(1);
        q.confirm_delete = None;
    }

    /// Start a fetch soon.
    fn queue_refetch(&mut self) {
        self.install_queue.last_fetch = None;
    }

    fn spawn_queue_fetch(&mut self) {
        let q = &mut self.install_queue;
        q.fetching = true;
        q.last_fetch = Some(Instant::now());
        let slot = Arc::clone(&q.slot);
        thread::spawn(move || {
            let result = iq::install_list().map_err(|e| e.describe());
            if let Ok(mut guard) = slot.lock() {
                *guard = Some(result);
            }
        });
    }

    /// Adopt results; refetch when due.
    pub fn poll_install_queue(&mut self) {
        let landed = self
            .install_queue
            .slot
            .lock()
            .ok()
            .and_then(|mut g| g.take());
        if let Some(result) = landed {
            self.install_queue.fetching = false;
            match result {
                Ok(entries) => self.apply_queue(entries),
                Err(e) if self.install_queue.show => self.install_queue.status = e,
                Err(_) => self.install_queue.scanned = true,
            }
        }
        let q = &self.install_queue;
        if q.fetching || self.items.is_empty() {
            return;
        }
        let wanted = !q.scanned || q.show || !q.adopted.is_empty();
        let due = q.last_fetch.is_none_or(|t| t.elapsed() >= REFRESH);
        if wanted && due {
            self.spawn_queue_fetch();
        }
    }

    /// Store entries and sync game cells.
    pub fn apply_queue(&mut self, entries: Vec<QueueEntry>) {
        self.track_adopted(&entries);
        self.adopt_entries(&entries);
        let q = &mut self.install_queue;
        q.scanned = true;
        q.loaded = true;
        q.index = q.index.min(entries.len().saturating_sub(1));
        q.entries = entries;
    }

    /// Show backlog entries on idle games.
    fn adopt_entries(&mut self, entries: &[QueueEntry]) {
        for entry in entries {
            let id = entry.app_id as i32;
            let Some(game) = self.items.iter().find(|g| g.id == id).cloned() else {
                continue;
            };
            let Some(text) = entry_status(entry) else {
                continue;
            };
            if cell_is_live(&game) {
                continue;
            }
            set_cell(&game, Some(text));
            if entry.is_running() {
                self.install_queue.adopted.insert(id);
            }
            if !self.install_controls.contains_key(&id) {
                self.begin_install(id, entry.library.clone());
            }
        }
    }

    /// Follow daemon installs we adopted.
    fn track_adopted(&mut self, entries: &[QueueEntry]) {
        let ids: Vec<i32> = self.install_queue.adopted.iter().copied().collect();
        for id in ids {
            let entry = entries.iter().find(|e| e.app_id as i32 == id);
            let game = self.items.iter().find(|g| g.id == id).cloned();
            match entry {
                Some(e) if e.is_running() => {
                    if let Some(g) = &game {
                        set_cell(g, entry_status(e));
                    }
                    continue;
                }
                Some(e) if e.is_resumable() => {
                    if let Some(g) = &game {
                        set_cell(g, entry_status(e));
                    }
                }
                Some(_) => {
                    if let Some(g) = &game {
                        set_cell(g, None);
                    }
                    self.forget_control(id);
                }
                None => {
                    // Finished or forgotten elsewhere.
                    if let Some(g) = &game {
                        set_cell(g, None);
                    }
                    self.forget_control(id);
                    self.request_game_refresh(id);
                }
            }
            self.install_queue.adopted.remove(&id);
        }
    }

    fn forget_control(&mut self, id: i32) {
        self.install_controls.remove(&id);
        self.install_library_choice.remove(&id);
    }

    /// Resume a paused game install.
    pub fn resume_install(&mut self, game: &Game) {
        let library = self.install_library_for(game.id).or_else(|| {
            self.install_queue
                .entries
                .iter()
                .find(|e| e.app_id as i32 == game.id)
                .and_then(|e| e.library.clone())
        });
        self.install_queue.adopted.remove(&game.id);
        let control = self.begin_install(game.id, library.clone());
        iq::spawn_resume(game.id as u32, library, game.status_counter(), control);
        self.queue_refetch();
    }

    fn queue_game(&self, app_id: u32) -> Option<Game> {
        self.items.iter().find(|g| g.id == app_id as i32).cloned()
    }

    /// `install pause` on the selection.
    pub fn queue_pause(&mut self) {
        let Some(entry) = self.install_queue.selected().cloned() else {
            return;
        };
        let id = entry.app_id as i32;
        if let Some(control) = self.install_controls.get(&id) {
            control.set(crate::interface::aurelia::InstallAction::Paused);
        }
        self.install_queue.status = match iq::install_pause(entry.app_id) {
            Ok(ControlOutcome::NotFound) => format!("{} is not queued", entry.label()),
            Ok(_) => format!("Pausing {}", entry.label()),
            Err(e) => e.describe(),
        };
        self.queue_refetch();
    }

    /// `install resume` on the selection.
    pub fn queue_resume(&mut self) {
        let Some(entry) = self.install_queue.selected().cloned() else {
            return;
        };
        if entry.is_running() {
            self.install_queue.status = format!("{} is already downloading", entry.label());
            return;
        }
        match self.queue_game(entry.app_id) {
            Some(game) => self.resume_install(&game),
            None => {
                let cell = Arc::new(Mutex::new(None));
                let control = crate::interface::aurelia::InstallControl::new();
                iq::spawn_resume(entry.app_id, entry.library.clone(), cell, control);
                self.queue_refetch();
            }
        }
        self.install_queue.status = format!("Resuming {}", entry.label());
    }

    /// `install stop` on the selection.
    pub fn queue_stop(&mut self) {
        let Some(entry) = self.install_queue.selected().cloned() else {
            return;
        };
        let id = entry.app_id as i32;
        if let Some(control) = self.install_controls.remove(&id) {
            control.set(crate::interface::aurelia::InstallAction::Stopped);
        }
        self.install_library_choice.remove(&id);
        self.install_queue.adopted.remove(&id);
        if let Some(game) = self.queue_game(entry.app_id) {
            set_cell(&game, None);
        }
        self.install_queue.status = match iq::install_stop(entry.app_id) {
            Ok(ControlOutcome::NotFound) => format!("{} is not queued", entry.label()),
            Ok(_) => format!("Stopped {}; kept as aborted", entry.label()),
            Err(e) => e.describe(),
        };
        self.queue_refetch();
    }

    /// `install forget`; delete asks twice.
    pub fn queue_forget(&mut self, delete_files: bool) {
        let Some(entry) = self.install_queue.selected().cloned() else {
            return;
        };
        if entry.is_running() {
            self.install_queue.status = format!("Stop {} before forgetting it", entry.label());
            return;
        }
        if delete_files && self.install_queue.confirm_delete != Some(entry.app_id) {
            self.install_queue.confirm_delete = Some(entry.app_id);
            self.install_queue.status = format!(
                "Press F again to forget {} and delete its partial files",
                entry.label()
            );
            return;
        }
        self.install_queue.confirm_delete = None;
        let id = entry.app_id as i32;
        self.install_queue.status = match iq::install_forget(entry.app_id, delete_files) {
            Ok(ControlOutcome::Forgotten(Some(dir))) => {
                format!("Forgot {}; deleted {dir}", entry.label())
            }
            Ok(_) => format!("Forgot {}", entry.label()),
            Err(e) => e.describe(),
        };
        self.forget_control(id);
        if let Some(game) = self.queue_game(entry.app_id)
            && !game.installed
        {
            set_cell(&game, None);
        }
        self.queue_refetch();
    }

    /// Re-read the download limit.
    pub fn refresh_download_limit(&mut self) {
        self.download_limit.kbit = iq::download_limit_show().ok();
    }

    pub fn begin_limit_edit(&mut self) {
        let current = self.download_limit.kbit.filter(|&k| k > 0);
        self.download_limit.input = Some(current.map(|k| k.to_string()).unwrap_or_default());
    }

    pub fn limit_input_push(&mut self, c: char) {
        if let Some(s) = self.download_limit.input.as_mut() {
            s.push(c);
        }
    }

    pub fn limit_input_pop(&mut self) {
        if let Some(s) = self.download_limit.input.as_mut() {
            s.pop();
        }
    }

    pub fn cancel_limit_edit(&mut self) {
        self.download_limit.input = None;
    }

    /// Save via `config download-limit`.
    pub fn commit_limit_edit(&mut self) {
        let Some(raw) = self.download_limit.input.take() else {
            return;
        };
        match iq::parse_limit(&raw) {
            Ok(kbit) => match iq::download_limit_set(kbit) {
                Ok(saved) => self.download_limit.kbit = Some(saved),
                Err(e) => self.notice = Some(format!("Download limit: {}", e.describe())),
            },
            Err(e) => self.notice = Some(format!("Download limit: {e}")),
        }
    }
}
