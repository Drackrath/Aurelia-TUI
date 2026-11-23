//! Account overlay loaded off-thread.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use super::Browser;
use crate::interface::aurelia::{self, AccountJson};

type Slot = Arc<Mutex<Option<(u64, Result<AccountJson, String>)>>>;

/// Background `aurelia account` fetch state.
#[derive(Default)]
pub struct AccountLoad {
    slot: Slot,
    generation: u64,
    /// Fetch still running.
    pub loading: bool,
    /// Last fetch error.
    pub error: Option<String>,
    /// When `account_info` was fetched.
    fetched_at: Option<Instant>,
    /// Reload feedback over cached data.
    pub status: Option<ReloadStatus>,
}

/// Feedback for a reload over cached data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReloadStatus {
    Reloading,
    Updated,
    Failed(String),
}

/// Cached account stays fresh this long.
pub const ACCOUNT_TTL: Duration = Duration::from_secs(600);

impl Browser {
    /// Open now; reuse a fresh cache.
    pub fn open_account(&mut self) {
        self.show_account = true;
        self.confirm_logout = false;
        let fresh = self
            .account_load
            .fetched_at
            .is_some_and(|t| t.elapsed() < ACCOUNT_TTL);
        if (fresh && self.account_info.is_some()) || self.account_load.loading {
            return;
        }
        self.refresh_account();
    }

    /// Refetch the account in background.
    pub fn refresh_account(&mut self) {
        let load = &mut self.account_load;
        load.generation = load.generation.wrapping_add(1);
        load.loading = true;
        load.error = None;
        // Keep cached rows while reloading.
        load.status = self
            .account_info
            .is_some()
            .then_some(ReloadStatus::Reloading);
        let (slot, generation) = (load.slot.clone(), load.generation);
        thread::spawn(move || {
            let result = aurelia::account().map_err(|e| e.to_string());
            if let Ok(mut s) = slot.lock() {
                *s = Some((generation, result));
            }
        });
    }

    /// Adopt a finished account fetch.
    pub fn poll_account(&mut self) {
        let Some((generation, result)) = self
            .account_load
            .slot
            .lock()
            .ok()
            .and_then(|mut s| s.take())
        else {
            return;
        };
        if generation != self.account_load.generation {
            return;
        }
        self.account_load.loading = false;
        match result {
            Ok(info) => {
                self.account_info = Some(info);
                self.account_load.fetched_at = Some(Instant::now());
                if self.account_load.status.is_some() {
                    self.account_load.status = Some(ReloadStatus::Updated);
                }
            }
            Err(error) => {
                self.notice = Some(format!("Account: {error}"));
                if self.account_load.status.is_some() {
                    self.account_load.status = Some(ReloadStatus::Failed(error.clone()));
                }
                self.account_load.error = Some(error);
            }
        }
    }

    /// Close; a running fetch still lands.
    pub fn close_account(&mut self) {
        self.show_account = false;
        self.confirm_logout = false;
        if !self.account_load.loading {
            self.account_load.status = None;
        }
    }

    /// Drop the cache, e.g. on logout.
    pub fn forget_account(&mut self) {
        self.account_info = None;
        self.account_load.fetched_at = None;
        self.account_load.status = None;
        self.account_load.generation = self.account_load.generation.wrapping_add(1);
        self.account_load.loading = false;
    }

    /// Age the cache (tests only).
    #[doc(hidden)]
    pub fn expire_account_for_test(&mut self) {
        self.account_load.fetched_at = Instant::now().checked_sub(ACCOUNT_TTL);
    }

    /// Install a finished result (tests only).
    #[doc(hidden)]
    pub fn finish_account_for_test(&mut self, result: Result<AccountJson, String>) {
        if let Ok(mut s) = self.account_load.slot.lock() {
            *s = Some((self.account_load.generation, result));
        }
    }
}
