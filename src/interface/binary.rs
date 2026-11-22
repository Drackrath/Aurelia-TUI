//! Chosen `aurelia` binary and stale-daemon handling.

use std::path::{Path, PathBuf};
use std::process;
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::interface::{aurelia, capabilities};
use crate::util::log::log;

/// Identity of an `aurelia` executable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinarySignature {
    pub path: String,
    pub size: u64,
    pub mtime: u64,
}

/// Oldest Aurelia that self-restarts stale daemons.
pub const SELF_RESTART_VERSION: (u32, u32, u32) = (0, 1, 35);

/// Expand `~` and trim the typed path.
pub fn expand(path: &str) -> String {
    let path = path.trim();
    shellexpand::tilde(path).into_owned()
}

/// Settings-chosen path, unless `AURELIA_BIN` overrides.
pub fn configured() -> Option<String> {
    if env_override().is_some() {
        return None;
    }
    Config::cached()
        .aurelia_bin
        .map(|p| expand(&p))
        .filter(|p| !p.is_empty())
}

/// Non-empty `AURELIA_BIN`, if set.
pub fn env_override() -> Option<String> {
    std::env::var("AURELIA_BIN").ok().filter(|v| !v.is_empty())
}

/// Find a bare program name on `PATH`.
fn which(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// Resolve `bin` to an existing file.
pub fn resolve(bin: &str) -> Option<PathBuf> {
    let path = Path::new(bin);
    if path.components().count() > 1 || path.is_absolute() {
        return path.is_file().then(|| path.to_path_buf());
    }
    which(bin)
}

/// Path, size and mtime of `bin`.
pub fn signature(bin: &str) -> Option<BinarySignature> {
    let path = resolve(bin)?;
    let path = std::fs::canonicalize(&path).unwrap_or(path);
    let meta = std::fs::metadata(&path).ok()?;
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_secs();
    Some(BinarySignature {
        path: path.to_string_lossy().into_owned(),
        size: meta.len(),
        mtime,
    })
}

/// What a binary change requires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonAction {
    /// Same binary, or first run.
    Nothing,
    /// Aurelia restarts its daemon itself.
    Notice,
    /// Old build: stop the daemon once.
    Stop,
}

/// Decide from last and current signatures.
pub fn daemon_action(
    last: Option<&BinarySignature>,
    current: &BinarySignature,
    version: Option<&str>,
) -> DaemonAction {
    match last {
        None => DaemonAction::Nothing,
        Some(last) if last == current => DaemonAction::Nothing,
        Some(_) if version.is_some_and(self_restarts) => DaemonAction::Notice,
        // Unrunnable binary: stop would fail.
        Some(_) if version.is_none() => DaemonAction::Nothing,
        Some(_) => DaemonAction::Stop,
    }
}

/// Whether `version` restarts stale daemons itself.
pub fn self_restarts(version: &str) -> bool {
    let core = version
        .trim_start_matches('v')
        .split('-')
        .next()
        .unwrap_or("");
    let parts: Vec<u32> = core.split('.').filter_map(|p| p.parse().ok()).collect();
    match parts.as_slice() {
        [major, minor, patch] => (*major, *minor, *patch) >= SELF_RESTART_VERSION,
        _ => false,
    }
}

/// Record binary first; acts once per change.
pub fn reconcile_daemon(config: &mut Config) -> Option<String> {
    let current = signature(&aurelia::bin())?;
    let version = capabilities::version();
    let action = daemon_action(config.last_binary.as_ref(), &current, version.as_deref());
    if config.last_binary.as_ref() != Some(&current) {
        let mut updated = config.clone();
        updated.last_binary = Some(current.clone());
        match updated.save() {
            Ok(()) => *config = updated,
            // Unsaved: never stop, avoids loops.
            Err(error) => {
                log!("binary record failed", error);
                return None;
            }
        }
    }
    match action {
        DaemonAction::Nothing => None,
        DaemonAction::Notice => Some(format!(
            "aurelia binary changed ({}); its daemon restarts on next use",
            current.path
        )),
        DaemonAction::Stop => Some(stop_daemon()),
    }
}

/// Run `aurelia daemon stop --json` once.
fn stop_daemon() -> String {
    let output = process::Command::new(aurelia::bin())
        .args(["daemon", "stop", "--json"])
        .stdin(process::Stdio::null())
        .stderr(process::Stdio::null())
        .output();
    match output {
        Ok(out) => {
            let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap_or_default();
            let killed = value.get("killed").and_then(|k| k.as_u64()).unwrap_or(0);
            format!("aurelia binary changed; stopped {killed} stale daemon(s)")
        }
        Err(error) => format!("aurelia binary changed; daemon stop failed: {error}"),
    }
}

/// Save typed path, re-probe, reconcile.
pub fn set_configured(config: &mut Config, typed: &str) -> Result<Option<String>, String> {
    let typed = typed.trim();
    let chosen = if typed.is_empty() {
        None
    } else {
        let expanded = expand(typed);
        if resolve(&expanded).is_none() {
            return Err(format!("aurelia binary not found: {expanded}"));
        }
        Some(typed.to_string())
    };
    let mut updated = config.clone();
    updated.aurelia_bin = chosen;
    updated.save().map_err(|e| format!("aurelia binary: {e}"))?;
    *config = updated;
    if let Some(env) = env_override() {
        return Ok(Some(format!("Saved; AURELIA_BIN ({env}) still wins")));
    }
    capabilities::probe();
    Ok(reconcile_daemon(config))
}

/// Settings row text.
pub fn describe(input: Option<&str>) -> String {
    if let Some(input) = input {
        return format!("{input}_");
    }
    if let Some(env) = env_override() {
        return format!("{env} (AURELIA_BIN)");
    }
    match Config::cached()
        .aurelia_bin
        .filter(|p| !p.trim().is_empty())
    {
        Some(path) => path,
        None => format!("auto ({})", aurelia::bin()),
    }
}
