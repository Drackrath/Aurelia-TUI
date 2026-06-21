//! Backend bridge to the `aurelia` command-line Steam launcher.
//!
//! This replaces the old `steamcmd` integration. Every operation shells out to
//! the `aurelia` binary with `--json` and parses its structured output. The
//! binary is expected to be on `PATH` (override with `AURELIA_BIN`), and the
//! user is expected to have authenticated once already with `aurelia login`
//! (the TUI only verifies the session via `aurelia login --health`).

use std::io::{BufRead, BufReader, Read, Write};
use std::process;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::thread;

use serde::Deserialize;

use crate::interface::game_status::GameStatus;
use crate::util::error::STError;
use crate::util::log::log;

/// The `aurelia` binary to invoke.
///
/// Resolution order: `AURELIA_BIN` (explicit override) → a sibling `Aurelia`
/// build next to this project (dev convenience, so the TUI works without
/// installing `aurelia` first) → bare `aurelia` (resolved on `PATH`).
pub fn bin() -> String {
    if let Ok(explicit) = std::env::var("AURELIA_BIN") {
        if !explicit.is_empty() {
            return explicit;
        }
    }

    let exe = if cfg!(windows) { "aurelia.exe" } else { "aurelia" };
    let mut roots: Vec<std::path::PathBuf> = Vec::new();
    // .../Aurelia-TUI/target/<profile>/aurelia-tui[.exe] -> workspace parent dir.
    if let Ok(cur) = std::env::current_exe() {
        if let Some(workspace_parent) = cur.ancestors().nth(4) {
            roots.push(workspace_parent.to_path_buf());
        }
    }
    // Or the parent of the current working directory (when run from the project).
    if let Ok(cwd) = std::env::current_dir() {
        if let Some(parent) = cwd.parent() {
            roots.push(parent.to_path_buf());
        }
    }
    for root in roots {
        for profile in ["release", "debug"] {
            let candidate = root.join("Aurelia").join("target").join(profile).join(exe);
            if candidate.is_file() {
                return candidate.to_string_lossy().into_owned();
            }
        }
    }

    "aurelia".to_string()
}

/// Session health, from `aurelia login --health --json`.
#[derive(Debug, Clone, Deserialize)]
pub struct HealthJson {
    #[serde(default)]
    pub logged_in: bool,
    #[serde(default)]
    pub account: Option<String>,
    #[serde(default)]
    pub steam_id: Option<u64>,
    #[serde(default)]
    pub daemon: bool,
}

/// Artwork URLs injected by `aurelia list --json`.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AssetsJson {
    #[serde(default)]
    pub header: Option<String>,
    #[serde(default)]
    pub capsule: Option<String>,
    #[serde(default)]
    pub hero: Option<String>,
    #[serde(default)]
    pub logo: Option<String>,
}

/// One library entry, from `aurelia list --json` (a `LibraryGame` plus the
/// `assets`/`store_url` fields the CLI bakes in for `--json` output).
#[derive(Debug, Clone, Deserialize)]
pub struct LibraryGameJson {
    pub app_id: u32,
    pub name: String,
    #[serde(default)]
    pub is_installed: bool,
    #[serde(default)]
    pub install_path: Option<String>,
    #[serde(default)]
    pub update_available: bool,
    #[serde(default)]
    pub is_owned: bool,
    #[serde(default)]
    pub is_family_shared: bool,
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub active_branch: Option<String>,
    #[serde(default)]
    pub assets: Option<AssetsJson>,
    #[serde(default)]
    pub store_url: Option<String>,
}

/// Store metadata, from `aurelia info <id> --json`. Only the fields the TUI
/// surfaces are captured; the key names match the CLI's `--json` output.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct InfoJson {
    #[serde(default)]
    pub app_id: u32,
    #[serde(default)]
    pub name: String,
    #[serde(default, rename = "type")]
    pub app_type: String,
    #[serde(default)]
    pub developers: Vec<String>,
    #[serde(default)]
    pub publishers: Vec<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub reviews: Option<String>,
}

/// A `qr_challenge` event line from `aurelia login --qr --json` (emitted on
/// stderr, re-emitted whenever Steam rotates the code).
#[derive(Debug, Deserialize)]
struct QrEvent {
    #[serde(default)]
    event: Option<String>,
    #[serde(default)]
    url: Option<String>,
}

/// Where a classic (username/password) login currently stands. Drives what the
/// TUI shows and when it prompts for a Steam Guard code.
#[derive(Debug, Clone, PartialEq)]
pub enum LoginPhase {
    Idle,
    Connecting,
    /// Login is blocked waiting for the user to approve it on their device, or
    /// to type a code (which may arrive next as `GuardCode`).
    AwaitingConfirmation,
    /// A typed Steam Guard code is required; the `String` is its kind
    /// (`"email"` or `"device"`).
    GuardCode(String),
    /// Approval must happen in the Steam Mobile app (no typed code).
    DeviceConfirmation,
    Success,
    Failed(String),
}

/// An NDJSON event line from `aurelia login --json` (on stderr).
#[derive(Debug, Deserialize)]
struct LoginEvent {
    #[serde(default)]
    event: Option<String>,
    #[serde(default, rename = "type")]
    guard_type: Option<String>,
}

/// A single NDJSON progress line from a streaming `install`/`update`/`verify`.
/// Progress events are emitted on **stderr**; the terminal result object goes
/// to **stdout**.
#[derive(Debug, Deserialize)]
struct ProgressJson {
    #[serde(default)]
    event: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    percent: Option<f64>,
}

/// Run `aurelia <args> --json`, returning the parsed stdout JSON value.
///
/// Errors surface as `{"error": "..."}` on stdout; that is translated into an
/// `STError::Problem`. stderr (tracing/diagnostics) is discarded.
fn run_json(args: &[&str]) -> Result<serde_json::Value, STError> {
    let output = process::Command::new(bin())
        .args(args)
        .arg("--json")
        .stdin(process::Stdio::null())
        .stderr(process::Stdio::null())
        .output()
        .map_err(STError::Process)?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Err(STError::Problem(format!(
            "aurelia produced no output for `{}` (is the `aurelia` binary on PATH?)",
            args.join(" ")
        )));
    }

    let value: serde_json::Value = serde_json::from_str(trimmed)?;
    if let Some(err) = value.get("error").and_then(|e| e.as_str()) {
        return Err(STError::Problem(err.to_string()));
    }
    Ok(value)
}

/// Report whether a Steam session is currently authenticated.
pub fn health() -> Result<HealthJson, STError> {
    let value = run_json(&["login", "--health"])?;
    Ok(serde_json::from_value(value)?)
}

/// Fetch the full library (`aurelia list --json`).
pub fn fetch_library() -> Result<Vec<LibraryGameJson>, STError> {
    let value = run_json(&["list"])?;
    Ok(serde_json::from_value(value)?)
}

/// Fetch store metadata for a single app (`aurelia info <id> --json`).
pub fn fetch_info(id: i32) -> Result<InfoJson, STError> {
    let value = run_json(&["info", &id.to_string()])?;
    Ok(serde_json::from_value(value)?)
}

/// Log in by QR code (`aurelia login --qr --json`), publishing each challenge
/// URL into `qr_cell` so the UI can render it. Blocks until the login resolves;
/// returns `Ok(())` once authenticated, `Err` on failure/timeout. Intended to
/// be run on a dedicated thread.
///
/// Forced to run standalone (`AURELIA_NO_DAEMON`) so it authenticates in this
/// process and writes `session.json`; any running daemon then reloads that
/// session by mtime on its next forwarded command.
pub fn login_qr(qr_cell: &Arc<Mutex<Option<String>>>) -> Result<(), STError> {
    let spawned = process::Command::new(bin())
        .args(["login", "--qr", "--json"])
        .env("AURELIA_NO_DAEMON", "1")
        .stdin(process::Stdio::null())
        .stdout(process::Stdio::piped())
        .stderr(process::Stdio::piped())
        .spawn();
    let mut child = spawned.map_err(STError::Process)?;

    // The terminal result object is the single line on stdout; drain it on a
    // helper thread while we consume the challenge events on stderr.
    let stdout = child.stdout.take();
    let result_handle = thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut out) = stdout {
            let _ = out.read_to_string(&mut buf);
        }
        buf
    });

    if let Some(stderr) = child.stderr.take() {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(event) = serde_json::from_str::<QrEvent>(line) {
                if event.event.as_deref() == Some("qr_challenge") {
                    if let Some(url) = event.url {
                        if let Ok(mut slot) = qr_cell.lock() {
                            *slot = Some(url);
                        }
                    }
                }
            }
        }
    }

    let result = result_handle.join().unwrap_or_default();
    let _ = child.wait();

    let value: serde_json::Value = serde_json::from_str(result.trim()).map_err(|_| {
        STError::Problem("QR login did not complete (no result from aurelia)".to_string())
    })?;
    if value.get("logged_in").and_then(|b| b.as_bool()) == Some(true) {
        Ok(())
    } else if let Some(err) = value.get("error").and_then(|e| e.as_str()) {
        Err(STError::Problem(err.to_string()))
    } else {
        Err(STError::Problem("QR login did not complete".to_string()))
    }
}

fn set_phase(phase: &Arc<Mutex<LoginPhase>>, value: LoginPhase) {
    if let Ok(mut slot) = phase.lock() {
        *slot = value;
    }
}

/// Classic username/password login (`aurelia login --json -u <user>`).
///
/// The password is passed via the `AURELIA_PASSWORD` environment variable so it
/// never appears in the process's argument list. Steam Guard codes are read by
/// `aurelia` from stdin: when a `guard_required` event arrives we publish a
/// `GuardCode` phase, block on `guard_rx` until the UI supplies the code, then
/// write it to the child's stdin. Runs standalone (`AURELIA_NO_DAEMON`) so it
/// authenticates here and writes `session.json`. Intended to run on a dedicated
/// thread; returns `Ok(())` once authenticated.
pub fn login_classic(
    username: &str,
    password: &str,
    phase: &Arc<Mutex<LoginPhase>>,
    guard_rx: Receiver<String>,
) -> Result<(), STError> {
    set_phase(phase, LoginPhase::Connecting);

    let spawned = process::Command::new(bin())
        .args(["login", "--json", "-u", username])
        .env("AURELIA_PASSWORD", password)
        .env("AURELIA_NO_DAEMON", "1")
        .stdin(process::Stdio::piped())
        .stdout(process::Stdio::piped())
        .stderr(process::Stdio::piped())
        .spawn();
    let mut child = spawned.map_err(STError::Process)?;

    let mut stdin = child.stdin.take();
    let stdout = child.stdout.take();
    let result_handle = thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut out) = stdout {
            let _ = out.read_to_string(&mut buf);
        }
        buf
    });

    if let Some(stderr) = child.stderr.take() {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let Ok(event) = serde_json::from_str::<LoginEvent>(line) else {
                continue;
            };
            match event.event.as_deref() {
                Some("awaiting_confirmation") => {
                    set_phase(phase, LoginPhase::AwaitingConfirmation)
                }
                Some("guard_required") => match event.guard_type.as_deref() {
                    Some("device_confirmation") => {
                        set_phase(phase, LoginPhase::DeviceConfirmation)
                    }
                    Some(kind) => {
                        set_phase(phase, LoginPhase::GuardCode(kind.to_string()));
                        // Block until the UI delivers the code, then feed it to
                        // aurelia's stdin (one line, as it expects).
                        if let Ok(code) = guard_rx.recv() {
                            if let Some(si) = stdin.as_mut() {
                                let _ = writeln!(si, "{}", code);
                                let _ = si.flush();
                            }
                            set_phase(phase, LoginPhase::Connecting);
                        }
                    }
                    None => {}
                },
                _ => {}
            }
        }
    }

    let result = result_handle.join().unwrap_or_default();
    // Close stdin so aurelia sees EOF if it is still reading.
    drop(stdin);
    let _ = child.wait();

    match serde_json::from_str::<serde_json::Value>(result.trim()) {
        Ok(value) if value.get("logged_in").and_then(|b| b.as_bool()) == Some(true) => {
            set_phase(phase, LoginPhase::Success);
            Ok(())
        }
        Ok(value) => {
            let msg = value
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("login failed")
                .to_string();
            set_phase(phase, LoginPhase::Failed(msg.clone()));
            Err(STError::Problem(msg))
        }
        Err(_) => {
            let msg = "login did not complete".to_string();
            set_phase(phase, LoginPhase::Failed(msg.clone()));
            Err(STError::Problem(msg))
        }
    }
}

/// Update a shared status cell, preserving any previously-known install dir/size.
fn set_status(status: &Arc<Mutex<Option<GameStatus>>>, msg: &str) {
    if let Ok(mut guard) = status.lock() {
        let next = GameStatus::msg(&guard, msg);
        *guard = Some(next);
    }
}

/// Install a game (`aurelia install <id> --json`), streaming progress into the
/// shared status cell. Blocks until the install finishes; intended to be run on
/// a dedicated thread.
pub fn install(id: i32, status: Arc<Mutex<Option<GameStatus>>>) {
    set_status(&status, "processing...");

    let spawned = process::Command::new(bin())
        .args(["install", &id.to_string(), "--json"])
        .stdin(process::Stdio::null())
        .stdout(process::Stdio::piped())
        .stderr(process::Stdio::piped())
        .spawn();

    let mut child = match spawned {
        Ok(child) => child,
        Err(err) => {
            set_status(&status, &format!("Failed: {}", err));
            log!("Failed to spawn aurelia install", id, err);
            return;
        }
    };

    // Drain stdout (the small terminal result object) on a helper thread so the
    // child never blocks writing it while we consume the larger stderr stream.
    let stdout = child.stdout.take();
    let result_handle = thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut out) = stdout {
            let _ = out.read_to_string(&mut buf);
        }
        buf
    });

    if let Some(stderr) = child.stderr.take() {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(event) = serde_json::from_str::<ProgressJson>(line) {
                if event.event.as_deref() == Some("progress") {
                    let label = match event.state.as_deref() {
                        Some("verifying") => "verifying",
                        Some("moving") => "moving",
                        Some("queued") => "queued",
                        _ => "downloading",
                    };
                    set_status(
                        &status,
                        &format!("{} {:.1}%", label, event.percent.unwrap_or(0.0)),
                    );
                }
            }
        }
    }

    let result = result_handle.join().unwrap_or_default();
    let _ = child.wait();

    match serde_json::from_str::<serde_json::Value>(result.trim()) {
        Ok(value) => {
            if let Some(err) = value.get("error").and_then(|e| e.as_str()) {
                set_status(&status, &format!("Failed: {}", err));
            } else if value.get("status").and_then(|s| s.as_str()) == Some("installed") {
                set_status(&status, "Installed!");
            } else {
                set_status(&status, "done");
            }
        }
        // No parseable result line: fall back on the exit status we already waited on.
        Err(_) => set_status(&status, "Installed!"),
    }
}

/// Uninstall a game (`aurelia uninstall <id> --json`). The game's Wine prefix /
/// compat data is left in place (no `--delete-prefix`). Blocks until the CLI
/// reports the result; the parsed value is ignored beyond error detection.
pub fn uninstall(app_id: i32) -> Result<(), STError> {
    run_json(&["uninstall", &app_id.to_string()])?;
    Ok(())
}

/// Launch a game and wait for it to exit (`aurelia play <id> --json`).
/// Intended to be run on a dedicated thread.
pub fn play(id: i32, status: Arc<Mutex<Option<GameStatus>>>) {
    set_status(&status, "launching...");

    let output = process::Command::new(bin())
        .args(["play", &id.to_string(), "--json"])
        .stdin(process::Stdio::null())
        .stderr(process::Stdio::null())
        .output();

    match output {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            match serde_json::from_str::<serde_json::Value>(stdout.trim()) {
                Ok(value) => {
                    if let Some(err) = value.get("error").and_then(|e| e.as_str()) {
                        set_status(&status, &format!("Failed: {}", err));
                    } else {
                        set_status(&status, "ran (finished)");
                    }
                }
                Err(_) => {
                    if output.status.success() {
                        set_status(&status, "ran (finished)");
                    } else {
                        set_status(&status, "Failed to launch");
                    }
                }
            }
        }
        Err(err) => {
            set_status(&status, &format!("failed to launch: {}", err));
            log!("Failed to spawn aurelia play", id, err);
        }
    }
}
