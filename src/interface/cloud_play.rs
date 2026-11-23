//! Steam Cloud sync around `aurelia play`.

use std::sync::{Arc, Mutex};

use crate::config::Config;
use crate::interface::aurelia::CloudDirection;
use crate::interface::cloud_status;
use crate::interface::cloud_sync::{self, ConflictPrompt};
use crate::interface::game_status::GameStatus;
use crate::interface::launch::{PlayOutcome, play_succeeded};

/// What to do around one launch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoSync {
    /// Sync down before, up after.
    Run,
    /// TUI `auto_cloud_sync` is off.
    TuiOff,
    /// Aurelia's switch is off here.
    GameOff,
}

/// Decide from both switches.
pub fn decide(tui_enabled: bool, game_effective: Option<bool>) -> AutoSync {
    if !tui_enabled {
        AutoSync::TuiOff
    } else if game_effective == Some(false) {
        AutoSync::GameOff
    } else {
        AutoSync::Run
    }
}

/// Whether to upload after this run.
pub fn launch_failed(outcome: &PlayOutcome) -> bool {
    !play_succeeded(outcome)
}

static REFRESH: Mutex<Vec<i32>> = Mutex::new(Vec::new());

/// Games whose library entry needs refreshing.
pub fn take_refresh_requests() -> Vec<i32> {
    REFRESH
        .lock()
        .map(|mut g| std::mem::take(&mut *g))
        .unwrap_or_default()
}

fn request_refresh(app_id: i32) {
    if let Ok(mut g) = REFRESH.lock()
        && !g.contains(&app_id)
    {
        g.push(app_id);
    }
}

fn set_status(status: &Arc<Mutex<Option<GameStatus>>>, msg: &str) {
    if let Ok(mut guard) = status.lock() {
        let next = GameStatus::msg(&guard, msg);
        *guard = Some(next);
    }
}

fn current_state(status: &Arc<Mutex<Option<GameStatus>>>) -> Option<String> {
    status
        .lock()
        .ok()
        .and_then(|g| g.as_ref().map(|s| s.state.clone()))
}

/// One automatic sync; returns a note.
pub fn auto_sync(app_id: i32, direction: CloudDirection) -> String {
    cloud_status::mark_syncing(app_id);
    let result = cloud_sync::sync_resolving(
        |resolve| cloud_sync::sync(app_id, direction, resolve),
        |first| {
            cloud_sync::ask(ConflictPrompt {
                app_id,
                direction,
                conflicts: first.conflicts.clone(),
            })
        },
    );
    match result {
        Ok(res) => {
            cloud_status::record_result(app_id, &res);
            res.summary()
        }
        Err(err) => {
            let msg = err.to_string();
            cloud_status::record_error(app_id, &msg);
            // Raw text could trip "Failed" badge.
            "error (see Cloud in detail)".to_string()
        }
    }
}

/// Wrap a blocking `launch` with syncs.
pub fn around_play(
    app_id: i32,
    status: &Arc<Mutex<Option<GameStatus>>>,
    launch: impl FnOnce() -> PlayOutcome,
) -> PlayOutcome {
    let plan = match decide(Config::cached().auto_cloud_sync, None) {
        AutoSync::Run => decide(
            true,
            cloud_sync::game(app_id)
                .ok()
                .map(|g| g.cloud_sync_effective),
        ),
        other => other,
    };

    if plan == AutoSync::Run {
        set_status(status, "syncing cloud saves (down)...");
        auto_sync(app_id, CloudDirection::Down);
    } else if plan == AutoSync::GameOff {
        set_status(
            status,
            "Aurelia Steam Cloud sync off for this game, skipping",
        );
    }

    let outcome = launch();

    // No upload after failed start.
    if launch_failed(&outcome) {
        return outcome;
    }
    let state = current_state(status).unwrap_or_default();
    match plan {
        AutoSync::Run => {
            set_status(status, &format!("{state} · syncing cloud saves (up)..."));
            let note = auto_sync(app_id, CloudDirection::Up);
            set_status(status, &format!("{state} · cloud {note}"));
        }
        AutoSync::GameOff => {
            set_status(
                status,
                &format!("{state} · Aurelia Steam Cloud sync off for this game"),
            );
        }
        AutoSync::TuiOff => {}
    }
    request_refresh(app_id);
    outcome
}
