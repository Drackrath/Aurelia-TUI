//! Launch flow around `aurelia play`.

use std::process;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::Deserialize;

use crate::config::Config;
use crate::interface::aurelia::{self, LaunchTargetJson, PlayOpts};
use crate::interface::game_status::GameStatus;
use crate::util::error::STError;
use crate::util::log::log;

/// Exit code of typed `timeout`.
pub const TIMEOUT_EXIT_CODE: i32 = 82;

/// Shorter `finished` runs count as early.
pub const EARLY_EXIT_SECS: u64 = 15;

// --- Saved target, Steam integration ---

/// Saved target id, `None` = automatic.
pub fn saved_target(config: &Config, app_id: i32) -> Option<&str> {
    config.launch_targets.get(&app_id).map(String::as_str)
}

/// Save target; `None` means automatic.
pub fn set_saved_target(config: &mut Config, app_id: i32, target: Option<&str>) {
    match target {
        Some(id) if !id.is_empty() => {
            config.launch_targets.insert(app_id, id.to_string());
        }
        _ => {
            config.launch_targets.remove(&app_id);
        }
    }
}

/// Whether `--steam` is on.
pub fn steam_integration(config: &Config, app_id: i32) -> bool {
    config.steam_integration.contains(&app_id)
}

/// Set the Steam-integration flag.
pub fn set_steam_integration(config: &mut Config, app_id: i32, on: bool) {
    config.steam_integration.retain(|id| *id != app_id);
    if on {
        config.steam_integration.push(app_id);
        config.steam_integration.sort_unstable();
    }
}

/// Persist the saved target.
pub fn store_saved_target(app_id: i32, target: Option<&str>) -> Result<(), STError> {
    let mut config = Config::cached();
    set_saved_target(&mut config, app_id, target);
    config.save()
}

/// Persist the game's Steam-integration flag.
pub fn store_steam_integration(app_id: i32, on: bool) -> Result<(), STError> {
    let mut config = Config::cached();
    set_steam_integration(&mut config, app_id, on);
    config.save()
}

// --- launch-targets ---

#[derive(Debug, Clone, Default, Deserialize)]
struct TargetsJson {
    #[serde(default)]
    targets: Vec<LaunchTargetJson>,
}

/// Parse `launch-targets --json` output.
pub fn parse_launch_targets(value: serde_json::Value) -> Result<Vec<LaunchTargetJson>, STError> {
    let parsed: TargetsJson = serde_json::from_value(value)?;
    Ok(parsed.targets)
}

/// `launch-targets` args, maybe `--steam`.
pub fn launch_targets_args(app_id: i32, steam: bool) -> Vec<String> {
    let mut args = vec!["launch-targets".to_string(), app_id.to_string()];
    if steam {
        args.push("--steam".to_string());
    }
    args
}

/// Run `aurelia launch-targets <id> [--steam]`.
pub fn launch_targets(app_id: i32, steam: bool) -> Result<Vec<LaunchTargetJson>, STError> {
    let args = launch_targets_args(app_id, steam);
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    parse_launch_targets(aurelia::run_json(&refs)?)
}

/// How plain Play picks its target.
#[derive(Debug, Clone)]
pub enum TargetChoice {
    /// Nothing saved: plain `play` picks.
    Automatic,
    /// The saved target, still available.
    Saved(LaunchTargetJson),
    /// Saved target gone; default runs.
    Fallback {
        saved: String,
        default: Option<LaunchTargetJson>,
    },
}

/// Saved if available, else default.
pub fn resolve_target(targets: &[LaunchTargetJson], saved: Option<&str>) -> TargetChoice {
    let Some(saved) = saved.filter(|s| !s.is_empty()) else {
        return TargetChoice::Automatic;
    };
    match targets.iter().find(|t| t.id == saved && t.available) {
        Some(t) => TargetChoice::Saved(t.clone()),
        None => TargetChoice::Fallback {
            saved: saved.to_string(),
            default: targets.iter().find(|t| t.default).cloned(),
        },
    }
}

/// Notice for an unavailable saved target.
pub fn fallback_notice(saved: &str, default: Option<&LaunchTargetJson>) -> String {
    match default {
        Some(t) => format!(
            "Saved launch target {saved} is unavailable; using the default ({}).",
            t.label()
        ),
        None => format!("Saved launch target {saved} is unavailable; using the default."),
    }
}

/// Plain Play options plus fallback notice.
pub fn plain_play_opts(
    steam: bool,
    saved: Option<&str>,
    targets: Result<Vec<LaunchTargetJson>, String>,
) -> (PlayOpts, Option<String>) {
    let default_opts = PlayOpts {
        steam,
        ..PlayOpts::default()
    };
    if saved.is_none() {
        return (default_opts, None);
    }
    let targets = match targets {
        Ok(targets) => targets,
        Err(e) => {
            let notice =
                format!("Could not check the saved launch target ({e}); using the default.");
            return (default_opts, Some(notice));
        }
    };
    match resolve_target(&targets, saved) {
        TargetChoice::Automatic => (default_opts, None),
        TargetChoice::Saved(t) => {
            let mut opts = t.play_opts();
            opts.steam |= steam;
            (opts, None)
        }
        TargetChoice::Fallback { saved, default } => (
            default_opts,
            Some(fallback_notice(&saved, default.as_ref())),
        ),
    }
}

/// Plain Play; returns the outcome.
pub fn play_saved(app_id: i32, status: Arc<Mutex<Option<GameStatus>>>) -> PlayOutcome {
    let config = Config::cached();
    let steam = steam_integration(&config, app_id);
    let saved = saved_target(&config, app_id).map(str::to_owned);
    let targets = match &saved {
        Some(_) => {
            set_status(&status, "resolving launch target...");
            launch_targets(app_id, steam).map_err(|e| e.to_string())
        }
        None => Ok(Vec::new()),
    };
    let (opts, notice) = plain_play_opts(steam, saved.as_deref(), targets);
    if let Some(notice) = notice {
        push_notice(notice);
    }
    aurelia::play_with(app_id, opts, status)
}

/// Play as… options plus integration.
pub fn play_target_opts(target: &LaunchTargetJson, steam: bool) -> PlayOpts {
    let mut opts = target.play_opts();
    opts.steam |= steam;
    opts
}

// --- play result ---

/// The `play --json` result object.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PlayResultJson {
    #[serde(default)]
    pub app_id: u32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub steam_forced: Option<String>,
    #[serde(default)]
    pub runtime: Option<String>,
}

/// A failed `play` run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayError {
    /// Aurelia's error `type` (`access_denied`, `timeout`, ...).
    pub kind: Option<String>,
    pub message: String,
    pub hint: Option<String>,
    pub exit_code: Option<i32>,
}

impl PlayError {
    /// Kind, or `timeout` for exit 82.
    pub fn effective_kind(&self) -> Option<&str> {
        match self.kind.as_deref() {
            Some(kind) => Some(kind),
            None if self.exit_code == Some(TIMEOUT_EXIT_CODE) => Some("timeout"),
            None => None,
        }
    }
}

/// How a `play` run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlayOutcome {
    /// The game ran and exited.
    Finished {
        name: String,
        steam_forced: Option<String>,
        runtime: Option<String>,
    },
    /// `finished` too soon; likely never started.
    ExitedEarly {
        name: String,
        secs: u64,
        steam_forced: Option<String>,
    },
    /// Typed error, bad exit, spawn failure.
    Failed(PlayError),
}

/// Gate post-game work on this.
pub fn play_succeeded(outcome: &PlayOutcome) -> bool {
    outcome.succeeded()
}

/// Human reason for a `steam_forced` value.
pub fn steam_forced_reason(reason: &str) -> &'static str {
    match reason {
        "family-shared" => "it is shared with you through Steam Family",
        "licence-unconfirmed" => "its licence could not be confirmed without Steam",
        _ => "this game requires it",
    }
}

impl PlayOutcome {
    /// True only for `Finished`.
    pub fn succeeded(&self) -> bool {
        matches!(self, PlayOutcome::Finished { .. })
    }

    /// Why Aurelia forced Steam.
    pub fn steam_forced(&self) -> Option<&str> {
        match self {
            PlayOutcome::Finished { steam_forced, .. }
            | PlayOutcome::ExitedEarly { steam_forced, .. } => steam_forced.as_deref(),
            PlayOutcome::Failed(_) => None,
        }
    }

    /// Status line; failures start `Failed`.
    pub fn status_text(&self) -> String {
        match self {
            PlayOutcome::Finished { .. } => "ran (finished)".to_string(),
            PlayOutcome::ExitedEarly { secs, .. } => {
                format!("Failed to start? The game exited after {secs}s")
            }
            PlayOutcome::Failed(err) => {
                let detail = match err.hint.as_deref() {
                    Some(hint) if !hint.is_empty() => format!("{} — {hint}", err.message),
                    _ => err.message.clone(),
                };
                match err.effective_kind() {
                    Some("access_denied") => format!("Failed: access denied: {detail}"),
                    Some("timeout") => format!("Failed: the game never started: {detail}"),
                    _ => format!("Failed: {detail}"),
                }
            }
        }
    }

    /// Optional footer notice.
    pub fn notice(&self, app_id: i32) -> Option<String> {
        let who = |name: &str| {
            if name.is_empty() {
                format!("app {app_id}")
            } else {
                name.to_string()
            }
        };
        match self {
            PlayOutcome::Finished {
                name,
                steam_forced: Some(reason),
                ..
            } => Some(format!(
                "Steam was started for {} because {}.",
                who(name),
                steam_forced_reason(reason)
            )),
            PlayOutcome::Finished { .. } => None,
            PlayOutcome::ExitedEarly { name, secs, .. } => Some(format!(
                "{} exited after {secs}s; it may have failed to start.",
                who(name)
            )),
            PlayOutcome::Failed(err) => Some(match err.effective_kind() {
                Some("access_denied") => format!("Can't launch app {app_id}: {}", err.message),
                Some("timeout") => {
                    format!("App {app_id} never started; check the in-Wine Steam window or retry.")
                }
                _ => format!("Launch of app {app_id} failed: {}", err.message),
            }),
        }
    }
}

/// Last JSON object on stdout.
fn last_json(stdout: &str) -> Option<serde_json::Value> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        return Some(value);
    }
    trimmed
        .lines()
        .rev()
        .find_map(|line| serde_json::from_str::<serde_json::Value>(line.trim()).ok())
        .filter(|v| v.is_object())
}

fn json_str(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// Classify a `play --json` run.
pub fn parse_play_output(stdout: &str, exit_code: Option<i32>, elapsed: Duration) -> PlayOutcome {
    let value = last_json(stdout);

    if let Some(value) = &value
        && let Some(message) = json_str(value, "error")
    {
        return PlayOutcome::Failed(PlayError {
            kind: json_str(value, "type"),
            message,
            hint: json_str(value, "hint"),
            exit_code,
        });
    }

    let result: PlayResultJson = value
        .clone()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();

    match exit_code {
        Some(0) => {}
        Some(code) => {
            return PlayOutcome::Failed(PlayError {
                kind: None,
                message: format!("aurelia play exited with code {code}"),
                hint: None,
                exit_code: Some(code),
            });
        }
        None => {
            return PlayOutcome::Failed(PlayError {
                kind: None,
                message: "aurelia play was terminated".to_string(),
                hint: None,
                exit_code: None,
            });
        }
    }

    // Non-`finished` status means failure.
    if !result.status.is_empty() && result.status != "finished" {
        return PlayOutcome::Failed(PlayError {
            kind: None,
            message: format!("play ended with status {}", result.status),
            hint: None,
            exit_code,
        });
    }

    let secs = elapsed.as_secs();
    if secs < EARLY_EXIT_SECS {
        return PlayOutcome::ExitedEarly {
            name: result.name,
            secs,
            steam_forced: result.steam_forced,
        };
    }
    PlayOutcome::Finished {
        name: result.name,
        steam_forced: result.steam_forced,
        runtime: result.runtime,
    }
}

fn set_status(status: &Arc<Mutex<Option<GameStatus>>>, msg: &str) {
    if let Ok(mut guard) = status.lock() {
        let next = GameStatus::msg(&guard, msg);
        *guard = Some(next);
    }
}

/// Run `play`, publish status, return outcome.
pub fn run_play(
    app_id: i32,
    args: &[&str],
    status: &Arc<Mutex<Option<GameStatus>>>,
) -> PlayOutcome {
    set_status(status, "launching...");
    let started = Instant::now();
    let output = crate::interface::aurelia_log::output(
        process::Command::new(aurelia::bin())
            .args(args)
            .stdin(process::Stdio::null()),
        &format!("play {app_id}"),
    );
    let outcome = match output {
        Ok(output) => parse_play_output(
            &String::from_utf8_lossy(&output.stdout),
            output.status.code(),
            started.elapsed(),
        ),
        Err(err) => {
            log!("Failed to spawn aurelia play", app_id, err);
            PlayOutcome::Failed(PlayError {
                kind: None,
                message: format!("could not start aurelia: {err}"),
                hint: None,
                exit_code: None,
            })
        }
    };
    set_status(status, &outcome.status_text());
    if let Some(notice) = outcome.notice(app_id) {
        push_notice(notice);
    }
    outcome
}

// --- Worker footer notices ---

static NOTICES: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Queue a footer notice.
pub fn push_notice(notice: String) {
    if let Ok(mut queue) = NOTICES.lock() {
        queue.push(notice);
    }
}

/// Take queued notices, joined.
pub fn take_notice() -> Option<String> {
    let mut queue = NOTICES.lock().ok()?;
    if queue.is_empty() {
        return None;
    }
    let joined = queue.join("  ");
    queue.clear();
    Some(joined)
}

// --- Steam runtime install ---

/// Where a `steam-runtime install` run stands.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum RuntimeInstallState {
    #[default]
    Idle,
    Running(Instant),
    Done(Result<(), String>),
}

/// Handle to `steam-runtime install`.
#[derive(Debug, Clone, Default)]
pub struct RuntimeInstall {
    state: Arc<Mutex<RuntimeInstallState>>,
}

impl RuntimeInstall {
    /// Start install; false if running.
    pub fn start(&self) -> bool {
        self.start_with(|| aurelia::steam_runtime_install().map_err(|e| e.to_string()))
    }

    /// `start` with a custom job.
    pub fn start_with<F>(&self, job: F) -> bool
    where
        F: FnOnce() -> Result<(), String> + Send + 'static,
    {
        {
            let Ok(mut state) = self.state.lock() else {
                return false;
            };
            if matches!(*state, RuntimeInstallState::Running(_)) {
                return false;
            }
            *state = RuntimeInstallState::Running(Instant::now());
        }
        let state = Arc::clone(&self.state);
        thread::spawn(move || {
            let result = job();
            if let Ok(mut guard) = state.lock() {
                *guard = RuntimeInstallState::Done(result);
            }
        });
        true
    }

    pub fn is_running(&self) -> bool {
        self.state
            .lock()
            .map(|s| matches!(*s, RuntimeInstallState::Running(_)))
            .unwrap_or(false)
    }

    /// Progress line while running.
    pub fn progress_text(&self) -> Option<String> {
        let state = self.state.lock().ok()?;
        match &*state {
            RuntimeInstallState::Running(since) => Some(runtime_progress_text(since.elapsed())),
            _ => None,
        }
    }

    /// Take a result, reset idle.
    pub fn take_finished(&self) -> Option<Result<(), String>> {
        let mut state = self.state.lock().ok()?;
        match std::mem::take(&mut *state) {
            RuntimeInstallState::Done(result) => Some(result),
            other => {
                *state = other;
                None
            }
        }
    }
}

/// "Play as…" overlay launch state.
#[derive(Debug, Clone, Default)]
pub struct LaunchUi {
    /// Game the overlay shows.
    pub app_id: i32,
    /// Its saved target id, if any.
    pub saved: Option<String>,
    /// Off-thread Steam runtime install.
    pub runtime: RuntimeInstall,
    /// Awaiting `y` for runtime install.
    pub confirm_runtime: bool,
}

/// Short tag for missing piece.
pub fn missing_short(target: &LaunchTargetJson) -> String {
    match target.missing.as_deref() {
        Some("build") => "no build".to_string(),
        Some("steam-runtime") => "needs Steam runtime".to_string(),
        Some("host-steam") => "needs host Steam".to_string(),
        Some(other) => format!("needs {other}"),
        None => "unavailable".to_string(),
    }
}

/// Full sentence for a missing piece.
pub fn missing_long(target: &LaunchTargetJson) -> String {
    let build = match target.platform.as_str() {
        "linux" => "Linux",
        "windows" => "Windows",
        other => other,
    };
    match target.missing.as_deref() {
        Some("build") => format!("the {build} build is not installed."),
        Some("steam-runtime") => "the Steam runtime (in-Wine Steam) is not installed.".to_string(),
        Some("host-steam") => "Steam is not installed on this system.".to_string(),
        Some(other) => format!("{other} is missing."),
        None => "it can't be launched right now.".to_string(),
    }
}

/// Confirm prompt for the runtime install.
pub const RUNTIME_CONFIRM: &str = "Install the Steam runtime (in-Wine Steam)? \
     Steam downloads its client; size unknown. [y] install, any other key cancels.";

/// Progress line with a spinner.
pub fn runtime_progress_text(elapsed: Duration) -> String {
    const SPIN: [char; 4] = ['|', '/', '-', '\\'];
    let secs = elapsed.as_secs();
    let spin = SPIN[(elapsed.as_millis() / 250) as usize % SPIN.len()];
    format!("{spin} Installing the Steam runtime (in-Wine Steam)... {secs}s")
}

#[cfg(test)]
#[path = "launch_tests.rs"]
mod tests;
