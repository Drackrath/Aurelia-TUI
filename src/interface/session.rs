//! Login, logout and session helpers.

use std::sync::{Arc, Mutex};
use std::thread;

use serde::Deserialize;

use crate::interface::aurelia::run_json;
use crate::interface::cli_json;
use crate::util::error::STError;

/// Outcome of `login --qr --json` stdout.
pub fn qr_result(stdout: &str) -> Result<(), STError> {
    let values = cli_json::values(stdout).unwrap_or_default();
    if values.is_empty() {
        return Err(STError::Problem(
            "QR login did not complete (no result from aurelia)".to_string(),
        ));
    }
    // Success: `logged_in` line, then `ok` result.
    let approved = values.iter().any(|v| {
        v.get("logged_in").and_then(|b| b.as_bool()) == Some(true)
            || (v.get("event").and_then(|e| e.as_str()) == Some("result")
                && v.get("ok").and_then(|b| b.as_bool()) == Some(true))
    });
    if approved {
        return Ok(());
    }
    match cli_json::parse(stdout, None) {
        Err(err) => Err(err),
        Ok(_) => Err(STError::Problem("QR login did not complete".to_string())),
    }
}

/// Outcome of `login --json -u` stdout.
pub fn classic_result(stdout: &str) -> Result<(), STError> {
    match cli_json::parse(stdout, None) {
        Ok(Some(v)) if v.get("logged_in").and_then(|b| b.as_bool()) == Some(true) => Ok(()),
        Ok(Some(_)) => Err(STError::Problem("login failed".to_string())),
        Ok(None) => Err(STError::Problem("login did not complete".to_string())),
        Err(STError::Cli(err)) => Err(STError::Cli(err)),
        Err(_) => Err(STError::Problem("login did not complete".to_string())),
    }
}

/// TUI advice for a session error.
pub fn session_advice(message: &str) -> Option<&'static str> {
    let lower = message.to_ascii_lowercase();
    if lower.contains("keyring") {
        Some("Unlock your keyring, then press [r] to re-check.")
    } else if lower.contains("login --reconnect") {
        Some("Actions → Reconnect Steam session.")
    } else {
        None
    }
}

/// Error text plus TUI advice.
pub fn with_advice(message: String) -> String {
    match session_advice(&message) {
        Some(advice) => format!("{message}\n {advice}"),
        None => message,
    }
}

/// `login --reconnect --json` result.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ReconnectJson {
    #[serde(default)]
    pub logged_in: bool,
    #[serde(default)]
    pub account: Option<String>,
    #[serde(default)]
    pub daemon: bool,
}

/// Status-line text for a reconnect.
pub fn reconnect_notice(result: Result<ReconnectJson, STError>) -> String {
    match result {
        Ok(r) if r.logged_in => match r.account {
            Some(a) => format!("Steam session reconnected ({a})"),
            None => "Steam session reconnected".to_string(),
        },
        Ok(_) => "Reconnect: not logged in — sign in again".to_string(),
        Err(err) => format!("Reconnect failed: {err}"),
    }
}

/// Re-establish the daemon's session.
pub fn reconnect() -> Result<ReconnectJson, STError> {
    let value = run_json(&["login", "--reconnect"])?;
    Ok(serde_json::from_value(value)?)
}

/// Off-thread reconnect, polled by the UI.
#[derive(Default)]
pub struct Reconnect {
    slot: Arc<Mutex<Option<String>>>,
    running: bool,
}

impl Reconnect {
    /// Start unless one is running.
    pub fn start(&mut self) -> bool {
        if self.running {
            return false;
        }
        self.running = true;
        let slot = self.slot.clone();
        thread::spawn(move || {
            let notice = reconnect_notice(reconnect());
            if let Ok(mut s) = slot.lock() {
                *s = Some(notice);
            }
        });
        true
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    /// Finished notice, taken once.
    pub fn poll(&mut self) -> Option<String> {
        let notice = self.slot.lock().ok()?.take()?;
        self.running = false;
        Some(notice)
    }
}

/// What `aurelia logout --json` reports.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct LogoutJson {
    #[serde(default)]
    pub logged_out: bool,
    /// `revoked`, `none` or `not_revoked`.
    #[serde(default)]
    pub token: Option<String>,
    /// Why revocation failed.
    #[serde(default)]
    pub token_error: Option<String>,
}

impl LogoutJson {
    /// One status line for the user.
    pub fn summary(&self) -> String {
        match self.token.as_deref() {
            Some("revoked") => "Logged out; refresh token revoked at Steam.".to_string(),
            Some("none") => "Logged out; no refresh token was stored.".to_string(),
            Some("not_revoked") => match self.token_error.as_deref() {
                Some(e) if !e.is_empty() => {
                    format!("Logged out, but Steam did not revoke the token: {e}")
                }
                _ => "Logged out, but Steam did not revoke the token.".to_string(),
            },
            _ => "Logged out.".to_string(),
        }
    }
}
