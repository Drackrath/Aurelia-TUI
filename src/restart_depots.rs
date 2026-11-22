//! Steam-restart, depot files, push state.

use std::sync::{Arc, Mutex};
use std::thread;

use crossterm::event::KeyCode;

use crate::browse::Browser;
use crate::interface::depot_files::{self, DepotFilesJson, DownloadOutcome, ManifestSel};
use crate::interface::steam_restart::{self, Relocate};
use crate::util::error::STError;

/// A refused relocation awaiting y/n.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RestartRetry {
    pub kind: Relocate,
    pub app_id: i32,
    pub library: String,
}

/// Which text field the browser edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileInput {
    Filter,
    Manifest,
    Target,
}

/// Download progress shared with its thread.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum DownloadState {
    #[default]
    Idle,
    Running(String),
    Done(DownloadOutcome),
}

/// The file being (or last) downloaded.
#[derive(Debug, Clone, Default)]
pub struct DownloadRequest {
    pub path: String,
    pub sha1: String,
    pub size: u64,
    pub out_dir: String,
}

/// One depot's manifest file browser.
#[derive(Debug, Default)]
pub struct FileBrowser {
    pub app_id: u32,
    pub depot_id: u32,
    pub depot_label: String,
    pub sel: ManifestSel,
    pub filter: String,
    pub listing: Option<DepotFilesJson>,
    pub error: String,
    pub index: usize,
    /// Field being edited, plus its text.
    pub input: Option<(FileInput, String)>,
    pub last_dir: String,
    pub request: DownloadRequest,
    pub download: Arc<Mutex<DownloadState>>,
}

impl FileBrowser {
    pub fn entry_count(&self) -> usize {
        self.listing.as_ref().map_or(0, |l| l.files.len())
    }

    pub fn selected(&self) -> Option<&depot_files::DepotFileJson> {
        self.listing.as_ref().and_then(|l| l.files.get(self.index))
    }

    pub fn download_state(&self) -> DownloadState {
        self.download.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// Download awaits overwrite answer?
    pub fn asks_overwrite(&self) -> bool {
        matches!(
            self.download_state(),
            DownloadState::Done(DownloadOutcome::Exists(_))
        )
    }

    fn next(&mut self) {
        let n = self.entry_count();
        if n > 0 {
            self.index = (self.index + 1).min(n - 1);
        }
    }

    fn previous(&mut self) {
        self.index = self.index.saturating_sub(1);
    }

    /// Fetch the listing (blocking).
    pub fn reload(&mut self) {
        self.apply_listing(depot_files::files(
            self.app_id,
            self.depot_id,
            &self.sel,
            &self.filter,
        ));
    }

    pub fn apply_listing(&mut self, result: Result<DepotFilesJson, STError>) {
        self.index = 0;
        match result {
            Ok(listing) => {
                self.error.clear();
                self.listing = Some(listing);
            }
            Err(err) => {
                self.error = format!("Failed: {err}");
                self.listing = None;
            }
        }
    }

    /// Start downloading in the background.
    fn start_download(&mut self, request: DownloadRequest, force: bool) {
        if matches!(self.download_state(), DownloadState::Running(_)) {
            return;
        }
        self.last_dir = request.out_dir.clone();
        self.request = request.clone();
        if let Ok(mut slot) = self.download.lock() {
            *slot = DownloadState::Running(request.path.clone());
        }
        let cell = self.download.clone();
        let (app_id, depot_id, sel) = (self.app_id, self.depot_id, self.sel.clone());
        thread::spawn(move || {
            let dir = depot_files::expand_dir(&request.out_dir);
            let result = depot_files::download(app_id, depot_id, &request.path, &sel, &dir, force);
            let outcome = depot_files::outcome(&request.sha1, request.size, result);
            if let Ok(mut slot) = cell.lock() {
                *slot = DownloadState::Done(outcome);
            }
        });
    }
}

/// This topic's transient UI state.
#[derive(Debug, Default)]
pub struct RestartDepots {
    pub restart_prompt: Option<RestartRetry>,
    /// Aurelia's never-pulled refusal text.
    pub force_push_prompt: Option<String>,
    pub files: Option<FileBrowser>,
}

impl RestartDepots {
    /// Whether keys route here first.
    pub fn wants_key(&self) -> bool {
        self.restart_prompt.is_some() || self.force_push_prompt.is_some() || self.files.is_some()
    }
}

/// Question for refused relocations.
pub const RESTART_QUESTION: &str = "Steam is running: restart Steam to apply? (y/n)";

/// Prompt if Steam-running refusal.
pub fn offer_restart(browser: &mut Browser, kind: Relocate, err: &STError) {
    if !steam_restart::is_steam_running_error(&err.to_string()) {
        return;
    }
    let (app_id, library) = match kind {
        Relocate::Move => (browser.move_app_id, browser.move_path.clone()),
        Relocate::Relink => (browser.relink_app_id, browser.relink_path.clone()),
        Relocate::Import => (browser.import_app_id, browser.import_path.clone()),
    };
    browser.restart_depots.restart_prompt = Some(RestartRetry {
        kind,
        app_id,
        library,
    });
}

fn relocate_status(browser: &mut Browser, kind: Relocate) -> &mut String {
    match kind {
        Relocate::Move => &mut browser.move_status,
        Relocate::Relink => &mut browser.relink_status,
        Relocate::Import => &mut browser.import_status,
    }
}

/// Re-run a refused relocation with `--restart-steam`.
fn retry_with_restart(browser: &mut Browser, retry: RestartRetry) {
    let result = steam_restart::relocate(retry.kind, retry.app_id, &retry.library, true);
    *relocate_status(browser, retry.kind) = match result {
        Ok(done) if done.steam_restarted => "done (Steam restarted)".to_string(),
        Ok(_) => "done".to_string(),
        Err(err) => format!("Failed: {err}"),
    };
}

/// DLC toggle; queues if needed.
pub fn toggle_dlc(browser: &mut Browser, enable: bool) {
    let Some(entry) = browser.selected_dlc() else {
        return;
    };
    let name = entry.display_name();
    match steam_restart::toggle_dlc(entry.app_id, enable) {
        Ok(true) => {
            browser.notice = Some(format!(
                "{name}: Steam is running, change pending until next launch"
            ))
        }
        Ok(false) => {}
        Err(err) => browser.notice = Some(format!("DLC change failed: {err}")),
    }
    let _ = browser.refresh_dlc();
}

/// Push; offer `--force` on refusal.
pub fn collections_push(browser: &mut Browser) {
    browser.collections_status = match steam_restart::collections_push(false) {
        Ok(done) => format!("Pushed {} collection(s) to Steam Cloud.", done.count),
        Err(err) => {
            let msg = err.to_string();
            if steam_restart::is_never_pulled_error(&msg) {
                browser.restart_depots.force_push_prompt = Some(msg.clone());
            }
            format!("Failed: {msg}")
        }
    };
}

/// Browse the top depot's files.
pub fn open_files(browser: &mut Browser) {
    let Some(game) = browser.selected() else {
        return;
    };
    let Some(depot) = browser.depots.get(browser.depots_scroll) else {
        return;
    };
    let label = if depot.name.is_empty() {
        depot.id.to_string()
    } else {
        format!("{} ({})", depot.name, depot.id)
    };
    let mut files = FileBrowser {
        app_id: game.id as u32,
        depot_id: depot.id as u32,
        depot_label: label,
        last_dir: depot_files::default_download_dir(),
        ..FileBrowser::default()
    };
    files.reload();
    browser.restart_depots.files = Some(files);
}

/// Route key to topmost overlay.
pub fn handle_key(browser: &mut Browser, key: KeyCode) {
    let yes = matches!(key, KeyCode::Char('y') | KeyCode::Char('Y'));
    if let Some(retry) = browser.restart_depots.restart_prompt.take() {
        if yes {
            retry_with_restart(browser, retry);
        }
        return;
    }
    if let Some(msg) = browser.restart_depots.force_push_prompt.take() {
        browser.collections_status = if yes {
            match steam_restart::collections_push(true) {
                Ok(done) => format!("Force-pushed {} collection(s) to Steam Cloud.", done.count),
                Err(err) => format!("Failed: {err}"),
            }
        } else {
            format!("Failed: {msg}")
        };
        return;
    }
    let Some(files) = browser.restart_depots.files.as_mut() else {
        return;
    };
    if files_key(files, key) {
        browser.restart_depots.files = None;
    }
}

/// File browser keys; true closes.
pub fn files_key(files: &mut FileBrowser, key: KeyCode) -> bool {
    if files.asks_overwrite() {
        if matches!(key, KeyCode::Char('y') | KeyCode::Char('Y')) {
            let request = files.request.clone();
            if let Ok(mut slot) = files.download.lock() {
                *slot = DownloadState::Idle;
            }
            files.start_download(request, true);
        } else if let Ok(mut slot) = files.download.lock() {
            *slot = DownloadState::Idle;
        }
        return false;
    }
    if let Some((field, mut text)) = files.input.take() {
        match key {
            KeyCode::Esc => {}
            KeyCode::Enter | KeyCode::Char('\n') => submit_input(files, field, text),
            KeyCode::Backspace => {
                text.pop();
                files.input = Some((field, text));
            }
            KeyCode::Char(c) => {
                text.push(c);
                files.input = Some((field, text));
            }
            _ => files.input = Some((field, text)),
        }
        return false;
    }
    match key {
        KeyCode::Esc | KeyCode::Char('q') => return true,
        KeyCode::Down | KeyCode::Char('j') => files.next(),
        KeyCode::Up | KeyCode::Char('k') => files.previous(),
        KeyCode::Char('/') => files.input = Some((FileInput::Filter, files.filter.clone())),
        KeyCode::Char('b') => files.input = Some((FileInput::Manifest, files.sel.label())),
        KeyCode::Enter | KeyCode::Char('\n') | KeyCode::Char('d')
            if files.selected().is_some_and(|f| f.is_file()) =>
        {
            files.input = Some((FileInput::Target, files.last_dir.clone()));
        }
        _ => {}
    }
    false
}

fn submit_input(files: &mut FileBrowser, field: FileInput, text: String) {
    match field {
        FileInput::Filter => {
            files.filter = text.trim().to_string();
            files.reload();
        }
        FileInput::Manifest => {
            files.sel = ManifestSel::parse(&text);
            files.reload();
        }
        FileInput::Target => {
            let Some(entry) = files.selected() else {
                return;
            };
            let out_dir = if text.trim().is_empty() {
                depot_files::default_download_dir()
            } else {
                text.trim().to_string()
            };
            let request = DownloadRequest {
                path: entry.path.clone(),
                sha1: entry.sha1.clone(),
                size: entry.size,
                out_dir,
            };
            files.start_download(request, false);
        }
    }
}
