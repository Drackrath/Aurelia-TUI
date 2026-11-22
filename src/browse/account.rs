//! Account overlay loaded off-thread.

use std::sync::{Arc, Mutex};
use std::thread;

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
}

impl Browser {
    /// Open now; fetch account in background.
    pub fn open_account(&mut self) {
        self.show_account = true;
        self.confirm_logout = false;
        self.account_info = None;
        let load = &mut self.account_load;
        load.generation = load.generation.wrapping_add(1);
        load.loading = true;
        load.error = None;
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
            Ok(info) => self.account_info = Some(info),
            Err(error) => {
                self.notice = Some(format!("Account: {error}"));
                self.account_load.error = Some(error);
            }
        }
    }

    /// Close and drop a pending fetch.
    pub fn close_account(&mut self) {
        self.show_account = false;
        self.confirm_logout = false;
        self.account_load.generation = self.account_load.generation.wrapping_add(1);
        self.account_load.loading = false;
    }

    /// Install a finished result (tests only).
    #[doc(hidden)]
    pub fn finish_account_for_test(&mut self, result: Result<AccountJson, String>) {
        if let Ok(mut s) = self.account_load.slot.lock() {
            *s = Some((self.account_load.generation, result));
        }
    }
}
