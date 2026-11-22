//! Install backlog, progress, typed errors.

use std::io::{BufRead, BufReader, Read};
use std::process;
use std::sync::{Arc, Mutex};
use std::thread;

use serde::Deserialize;

use crate::interface::aurelia::{InstallAction, InstallControl, bin};
use crate::interface::game_status::GameStatus;

/// One `install list --json` row.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct QueueEntry {
    pub app_id: u32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub downloaded_bytes: u64,
    #[serde(default)]
    pub total_bytes: u64,
    #[serde(default)]
    pub percent: f64,
    /// Live text or last failure.
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub is_downloading: bool,
    /// running, queued, paused, aborted, interrupted.
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub library: Option<String>,
    #[serde(default)]
    pub error_type: Option<String>,
    #[serde(default)]
    pub added_at: Option<u64>,
    #[serde(default)]
    pub updated_at: Option<u64>,
}

impl QueueEntry {
    pub fn is_running(&self) -> bool {
        self.state == "running"
    }

    /// Paused, interrupted or queued.
    pub fn is_resumable(&self) -> bool {
        matches!(self.state.as_str(), "paused" | "interrupted" | "queued")
    }

    /// Clamped percent from bytes.
    pub fn progress_percent(&self) -> f64 {
        percent(self.downloaded_bytes, self.total_bytes).unwrap_or(self.percent.min(100.0))
    }

    /// Name, or app id fallback.
    pub fn label(&self) -> String {
        if self.name.is_empty() {
            format!("app {}", self.app_id)
        } else {
            self.name.clone()
        }
    }

    /// Short reason for the overlay.
    pub fn note(&self) -> String {
        if let Some(kind) = self.error_type.as_deref().map(ErrorKind::parse)
            && (kind != ErrorKind::Unknown || self.status.is_empty())
        {
            return kind.title().to_string();
        }
        self.status.clone()
    }
}

/// Parse an `install list` value.
pub fn parse_list(value: serde_json::Value) -> Result<Vec<QueueEntry>, String> {
    serde_json::from_value(value).map_err(|e| format!("bad install list: {e}"))
}

/// Aurelia `ErrorKind`, snake_case on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    DiskFull,
    Offline,
    Cancelled,
    NetworkTimeout,
    RateLimited,
    AuthRequired,
    AccessDenied,
    NotFound,
    InvalidInput,
    SteamConfigMismatch,
    Timeout,
    InputClosed,
    SourceChanged,
    Unknown,
}

impl ErrorKind {
    pub fn parse(raw: &str) -> ErrorKind {
        match raw {
            "disk_full" => Self::DiskFull,
            "offline" => Self::Offline,
            "cancelled" => Self::Cancelled,
            "network_timeout" => Self::NetworkTimeout,
            "rate_limited" => Self::RateLimited,
            "auth_required" | "login_denied" | "login_expired" => Self::AuthRequired,
            "access_denied" | "privacy_restricted" => Self::AccessDenied,
            "not_found" => Self::NotFound,
            "invalid_input" => Self::InvalidInput,
            "steam_config_mismatch" => Self::SteamConfigMismatch,
            "timeout" => Self::Timeout,
            "input_closed" => Self::InputClosed,
            "source_changed" => Self::SourceChanged,
            _ => Self::Unknown,
        }
    }

    /// Short headline per kind.
    pub fn title(self) -> &'static str {
        match self {
            Self::DiskFull => "Disk full",
            Self::Offline => "Offline",
            Self::Cancelled => "Cancelled",
            Self::NetworkTimeout => "Network timed out",
            Self::RateLimited => "Rate limited by Steam",
            Self::AuthRequired => "Not signed in",
            Self::AccessDenied => "Access denied",
            Self::NotFound => "Not found",
            Self::InvalidInput => "Invalid request",
            Self::SteamConfigMismatch => "Steam config mismatch",
            Self::Timeout => "Timed out",
            Self::InputClosed => "Input closed",
            Self::SourceChanged => "Steam response changed",
            Self::Unknown => "Error",
        }
    }

    /// What the user can do.
    pub fn advice(self) -> Option<&'static str> {
        match self {
            Self::DiskFull => Some("free space or pick another library, then resume"),
            Self::Offline => Some("reconnect, then resume from the install queue"),
            Self::Cancelled => Some("it stays in the install queue"),
            Self::NetworkTimeout => Some("check the connection, then resume"),
            Self::RateLimited => Some("wait a little, then resume"),
            Self::AuthRequired => Some("sign in again"),
            _ => None,
        }
    }
}

/// A typed CLI error.
#[derive(Debug, Clone, PartialEq)]
pub struct InstallError {
    pub kind: ErrorKind,
    pub message: String,
    pub hint: Option<String>,
}

impl InstallError {
    fn plain(message: impl Into<String>) -> Self {
        InstallError {
            kind: ErrorKind::Unknown,
            message: message.into(),
            hint: None,
        }
    }

    /// One readable line.
    pub fn describe(&self) -> String {
        let mut out = match self.kind {
            ErrorKind::Unknown => self.message.clone(),
            kind => format!("{}: {}", kind.title(), self.message),
        };
        let extra = self.kind.advice().map(str::to_string).or(self.hint.clone());
        if let Some(extra) = extra.filter(|e| !e.is_empty()) {
            out.push_str(&format!(" ({extra})"));
        }
        out
    }
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.describe())
    }
}

/// The `{error,type,hint}` object, if any.
pub fn parse_error(value: &serde_json::Value) -> Option<InstallError> {
    let message = value.get("error")?.as_str()?.to_string();
    let kind = value
        .get("type")
        .and_then(|t| t.as_str())
        .map(ErrorKind::parse)
        .unwrap_or(ErrorKind::Unknown);
    let hint = value
        .get("hint")
        .and_then(|h| h.as_str())
        .map(str::to_string);
    Some(InstallError {
        kind,
        message,
        hint,
    })
}

/// Whole stdout, else its last line.
pub fn parse_stdout(stdout: &str) -> Option<serde_json::Value> {
    let trimmed = stdout.trim();
    if let Ok(v) = serde_json::from_str(trimmed) {
        return Some(v);
    }
    trimmed
        .lines()
        .rev()
        .find_map(|l| serde_json::from_str(l.trim()).ok())
}

/// Run `aurelia <args> --json`.
fn run(args: &[&str]) -> Result<serde_json::Value, InstallError> {
    let output = process::Command::new(bin())
        .args(args)
        .arg("--json")
        .stdin(process::Stdio::null())
        .stderr(process::Stdio::null())
        .output()
        .map_err(|e| InstallError::plain(format!("could not run aurelia: {e}")))?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let value = parse_stdout(&stdout).ok_or_else(|| {
        InstallError::plain(format!(
            "aurelia produced no output for `{}`",
            args.join(" ")
        ))
    })?;
    match parse_error(&value) {
        Some(err) => Err(err),
        None => Ok(value),
    }
}

/// `aurelia install list --json`.
pub fn install_list() -> Result<Vec<QueueEntry>, InstallError> {
    parse_list(run(&["install", "list"])?).map_err(InstallError::plain)
}

/// Reply to pause, stop or forget.
#[derive(Debug, Clone, PartialEq)]
pub enum ControlOutcome {
    /// Running install is stopping.
    Signalled,
    /// Backlog entry relabelled.
    Marked(String),
    /// Removed; deleted dir if any.
    Forgotten(Option<String>),
    NotFound,
}

/// Classify a control reply.
pub fn parse_control(value: &serde_json::Value) -> ControlOutcome {
    let field = |k: &str| value.get(k).and_then(|v| v.as_str());
    match (field("event"), field("status")) {
        (Some("not_found"), _) => ControlOutcome::NotFound,
        (Some("pausing" | "stopping"), _) => ControlOutcome::Signalled,
        (_, Some("forgotten")) => ControlOutcome::Forgotten(field("deleted").map(str::to_string)),
        _ => ControlOutcome::Marked(field("state").unwrap_or_default().to_string()),
    }
}

/// `aurelia install pause <id>`.
pub fn install_pause(app_id: u32) -> Result<ControlOutcome, InstallError> {
    Ok(parse_control(&run(&[
        "install",
        "pause",
        &app_id.to_string(),
    ])?))
}

/// `aurelia install stop <id>`.
pub fn install_stop(app_id: u32) -> Result<ControlOutcome, InstallError> {
    Ok(parse_control(&run(&[
        "install",
        "stop",
        &app_id.to_string(),
    ])?))
}

/// `aurelia install forget <id> [--delete-files]`.
pub fn install_forget(app_id: u32, delete_files: bool) -> Result<ControlOutcome, InstallError> {
    let id = app_id.to_string();
    let mut args = vec!["install", "forget", &id];
    if delete_files {
        args.push("--delete-files");
    }
    Ok(parse_control(&run(&args)?))
}

/// One NDJSON progress event.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct ProgressEvent {
    #[serde(default)]
    pub event: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    /// Includes `reused_bytes`; never add.
    #[serde(default)]
    pub bytes_downloaded: u64,
    /// Verified from disk, not network.
    #[serde(default)]
    pub reused_bytes: u64,
    #[serde(default)]
    pub total_bytes: u64,
    #[serde(default)]
    pub percent: Option<f64>,
    /// Network bytes per second.
    #[serde(default)]
    pub speed_bps: Option<u64>,
    #[serde(default)]
    pub eta_seconds: Option<u64>,
}

/// Progress event from one line.
pub fn parse_progress(line: &str) -> Option<ProgressEvent> {
    let event: ProgressEvent = serde_json::from_str(line.trim()).ok()?;
    (event.event.as_deref() == Some("progress")).then_some(event)
}

fn percent(done: u64, total: u64) -> Option<f64> {
    (total > 0).then(|| (done as f64 / total as f64 * 100.0).min(100.0))
}

/// Status text for one event.
pub fn progress_status(p: &ProgressEvent) -> String {
    let label = match p.state.as_deref() {
        Some("verifying") => "verifying",
        Some("moving") => "moving",
        Some("queued") => "queued",
        _ => "downloading",
    };
    let pct = percent(p.bytes_downloaded, p.total_bytes)
        .or(p.percent.map(|v| v.min(100.0)))
        .unwrap_or(0.0);
    let mut out = format!("{label} {pct:.1}%");
    if let Some(speed) = p.speed_bps.filter(|&s| s > 0) {
        out.push_str(&format!(" · {}/s", human_bytes(speed)));
    }
    if let Some(eta) = p.eta_seconds {
        out.push_str(&format!(" · ETA {}", format_eta(eta)));
    }
    if p.reused_bytes > 0 {
        out.push_str(&format!(" · {} reused", human_bytes(p.reused_bytes)));
    }
    out
}

/// Compact ETA like `3m 12s`.
pub fn format_eta(secs: u64) -> String {
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{h}h {m:02}m")
    } else if m > 0 {
        format!("{m}m {s:02}s")
    } else {
        format!("{s}s")
    }
}

/// Decimal units, matching limit hints.
pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "kB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// Final status from result stdout.
pub fn result_status(stdout: &str) -> Option<String> {
    let Some(value) = parse_stdout(stdout) else {
        return Some("Installed!".to_string());
    };
    if let Some(err) = parse_error(&value) {
        if err.kind == ErrorKind::Cancelled && err.message.contains("paused") {
            // Paused elsewhere; keep percent.
            return None;
        }
        return Some(format!("Failed: {}", err.describe()));
    }
    Some(match value.get("status").and_then(|s| s.as_str()) {
        Some("installed") => "Installed!".to_string(),
        _ => "done".to_string(),
    })
}

/// Whether stdout says `not_found`.
fn is_not_found(stdout: &str) -> bool {
    parse_stdout(stdout)
        .and_then(|v| parse_error(&v))
        .is_some_and(|e| e.kind == ErrorKind::NotFound)
}

fn set_status(status: &Arc<Mutex<Option<GameStatus>>>, msg: &str) {
    if let Ok(mut guard) = status.lock() {
        let next = GameStatus::msg(&guard, msg);
        *guard = Some(next);
    }
}

/// `paused 42.0%` from last status.
pub fn paused_text(previous: &str, suffix: Option<&str>) -> String {
    let mut out = previous
        .split_whitespace()
        .find(|t| t.ends_with('%'))
        .map(|p| format!("paused {p}"))
        .unwrap_or_else(|| "paused".to_string());
    if let Some(s) = suffix.filter(|s| !s.is_empty()) {
        out.push_str(&format!(" · {s}"));
    }
    out
}

/// Freeze the cell as paused.
pub fn set_paused(status: &Arc<Mutex<Option<GameStatus>>>, suffix: Option<&str>) {
    if let Ok(mut guard) = status.lock() {
        let prev = guard.as_ref().map(|s| s.state.clone()).unwrap_or_default();
        let next = GameStatus::msg(&guard, &paused_text(&prev, suffix));
        *guard = Some(next);
    }
}

/// Run a streaming install; returns stdout.
fn stream(args: &[String], status: &Arc<Mutex<Option<GameStatus>>>) -> Result<String, String> {
    let mut cmd = process::Command::new(bin());
    cmd.args(args);
    if !args.iter().any(|a| a == "--json") {
        cmd.arg("--json");
    }
    let mut child = cmd
        .stdin(process::Stdio::null())
        .stdout(process::Stdio::piped())
        .stderr(process::Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let stdout = child.stdout.take();
    let result = thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut out) = stdout {
            let _ = out.read_to_string(&mut buf);
        }
        buf
    });
    if let Some(stderr) = child.stderr.take() {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if let Some(event) = parse_progress(&line) {
                set_status(status, &progress_status(&event));
            }
        }
    }
    let out = result.join().unwrap_or_default();
    let _ = child.wait();
    Ok(out)
}

/// Settle the cell after the process.
fn finish(stdout: &str, status: &Arc<Mutex<Option<GameStatus>>>, control: &InstallControl) {
    match control.get() {
        InstallAction::Paused => set_paused(status, None),
        InstallAction::Stopped => {
            if let Ok(mut guard) = status.lock() {
                *guard = None;
            }
        }
        InstallAction::Running => match result_status(stdout) {
            Some(text) => set_status(status, &text),
            None => set_paused(status, None),
        },
    }
}

/// Stream `aurelia <args>` into `status`.
pub fn run_install(
    args: Vec<String>,
    status: Arc<Mutex<Option<GameStatus>>>,
    control: InstallControl,
) {
    set_status(&status, "processing...");
    match stream(&args, &status) {
        Ok(out) => finish(&out, &status, &control),
        Err(e) => set_status(&status, &format!("Failed: {e}")),
    }
}

/// Resume; fresh install if not queued.
pub fn resume(
    app_id: u32,
    library: Option<String>,
    status: Arc<Mutex<Option<GameStatus>>>,
    control: InstallControl,
) {
    set_status(&status, "processing...");
    let args = vec!["install".into(), "resume".into(), app_id.to_string()];
    let out = match stream(&args, &status) {
        Ok(out) => out,
        Err(e) => return set_status(&status, &format!("Failed: {e}")),
    };
    if !is_not_found(&out) || control.get() != InstallAction::Running {
        return finish(&out, &status, &control);
    }
    let mut args = vec!["install".to_string(), app_id.to_string()];
    if let Some(lib) = library {
        args.push("--library".into());
        args.push(lib);
    }
    // Keep picker platform and language.
    args.extend(control.options().args());
    run_install(args, status, control);
}

/// Spawn [`resume`] on a thread.
pub fn spawn_resume(
    app_id: u32,
    library: Option<String>,
    status: Arc<Mutex<Option<GameStatus>>>,
    control: InstallControl,
) {
    thread::spawn(move || resume(app_id, library, status, control));
}

/// Settings row state.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DownloadLimit {
    /// Current kbit/s; 0 is off.
    pub kbit: Option<u64>,
    /// Edit buffer while typing.
    pub input: Option<String>,
}

/// Parse input like Aurelia does.
pub fn parse_limit(raw: &str) -> Result<u64, String> {
    let value = raw.trim();
    if value.is_empty()
        || ["off", "none", "unlimited"]
            .iter()
            .any(|w| value.eq_ignore_ascii_case(w))
    {
        return Ok(0);
    }
    value
        .parse::<u64>()
        .map_err(|_| format!("`{raw}` is not a number of kbit/s or `off`"))
}

/// Rate hint, e.g. `1.0 MB/s`.
pub fn limit_rate(kbit: u64) -> String {
    let bytes = kbit.saturating_mul(1000) / 8;
    if bytes >= 1_000_000 {
        format!("{:.1} MB/s", bytes as f64 / 1_000_000.0)
    } else {
        format!("{:.1} kB/s", bytes as f64 / 1_000.0)
    }
}

/// Row text for a limit.
pub fn describe_limit(kbit: u64) -> String {
    if kbit == 0 {
        "off (unlimited)".to_string()
    } else {
        format!("{kbit} kbit/s (≈ {})", limit_rate(kbit))
    }
}

/// Read `download_limit_kbit` from JSON.
pub fn parse_limit_json(value: &serde_json::Value) -> Option<u64> {
    value.get("download_limit_kbit")?.as_u64()
}

/// `aurelia config download-limit --json`.
pub fn download_limit_show() -> Result<u64, InstallError> {
    let value = run(&["config", "download-limit"])?;
    parse_limit_json(&value).ok_or_else(|| InstallError::plain("no download_limit_kbit"))
}

/// `aurelia config download-limit <kbit>`.
pub fn download_limit_set(kbit: u64) -> Result<u64, InstallError> {
    let value = run(&["config", "download-limit", &kbit.to_string()])?;
    parse_limit_json(&value).ok_or_else(|| InstallError::plain("no download_limit_kbit"))
}
