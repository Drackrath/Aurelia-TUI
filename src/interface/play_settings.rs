//! Launch options, prefix, shaders, anti-cheat.

use std::io::Read;
use std::process;
use std::sync::{Arc, Mutex};
use std::thread;

use serde::Deserialize;

use crate::interface::aurelia::{bin, run_json};
use crate::interface::game_status::GameStatus;
use crate::util::error::STError;

/// Launch fields of `config game`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct GameLaunchJson {
    #[serde(default)]
    pub launch_options: String,
    #[serde(default)]
    pub prefix_dir: Option<String>,
    #[serde(default)]
    pub wine_prefix: Option<String>,
}

/// Play fields of `config show`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct PlayConfigJson {
    /// `"auto"` or `"off"`.
    #[serde(default)]
    pub shader_precache: Option<String>,
    /// Aurelia defaults this on.
    #[serde(default = "default_true")]
    pub anticheat_auto_install: bool,
}

fn default_true() -> bool {
    true
}

impl Default for PlayConfigJson {
    fn default() -> Self {
        PlayConfigJson {
            shader_precache: None,
            anticheat_auto_install: true,
        }
    }
}

impl PlayConfigJson {
    /// Whether shader pre-caching is on.
    pub fn shaders_auto(&self) -> bool {
        self.shader_precache.as_deref() != Some("off")
    }
}

/// A Proton anti-cheat runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AntiCheat {
    Eac,
    BattlEye,
}

impl AntiCheat {
    /// The `proton anticheat` argument.
    pub fn arg(self) -> &'static str {
        match self {
            AntiCheat::Eac => "eac",
            AntiCheat::BattlEye => "battleye",
        }
    }

    /// Short human label.
    pub fn label(self) -> &'static str {
        match self {
            AntiCheat::Eac => "EAC runtime",
            AntiCheat::BattlEye => "BattlEye runtime",
        }
    }
}

/// Expand `~` in prefix paths.
pub fn expand_prefix(raw: &str) -> String {
    shellexpand::tilde(raw.trim()).into_owned()
}

/// Set launch options; empty clears.
pub fn set_launch_options(app_id: i32, line: &str) -> Result<(), STError> {
    let id = app_id.to_string();
    let line = line.trim();
    if line.is_empty() {
        run_json(&["config", "game", &id, "--no-launch-options"])?;
    } else {
        // `=` form keeps leading dashes literal.
        let arg = format!("--launch-options={line}");
        run_json(&["config", "game", &id, &arg])?;
    }
    Ok(())
}

/// Set Wine prefix; empty clears.
pub fn set_prefix(app_id: i32, dir: &str) -> Result<(), STError> {
    let id = app_id.to_string();
    let dir = expand_prefix(dir);
    if dir.is_empty() {
        run_json(&["config", "game", &id, "--no-prefix"])?;
    } else {
        run_json(&["config", "game", &id, "--prefix", &dir])?;
    }
    Ok(())
}

/// Set `config shader-precache auto|off`.
pub fn set_shader_precache(auto: bool) -> Result<(), STError> {
    run_json(&[
        "config",
        "shader-precache",
        if auto { "auto" } else { "off" },
    ])?;
    Ok(())
}

/// Set `config anticheat-auto-install on|off`.
pub fn set_anticheat_auto_install(on: bool) -> Result<(), STError> {
    run_json(&[
        "config",
        "anticheat-auto-install",
        if on { "on" } else { "off" },
    ])?;
    Ok(())
}

/// One NDJSON line.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct EventJson {
    #[serde(default)]
    pub event: Option<String>,
    #[serde(default)]
    pub state: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub percent: Option<f64>,
    #[serde(default)]
    pub done: Option<u64>,
    #[serde(default)]
    pub total: Option<u64>,
    #[serde(default)]
    pub databases: Option<u64>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub hint: Option<String>,
}

impl EventJson {
    fn error_text(&self) -> Option<String> {
        let err = self.error.as_ref()?;
        Some(match self.hint.as_deref() {
            Some(h) if !h.is_empty() => format!("{err} ({h})"),
            _ => err.clone(),
        })
    }
}

/// Status for a shaders event.
pub fn shader_event_status(line: &str) -> Option<String> {
    let ev: EventJson = serde_json::from_str(line.trim()).ok()?;
    if ev.event.as_deref() != Some("shaders") {
        return None;
    }
    match ev.state.as_deref()? {
        "started" => Some(format!(
            "shaders: starting ({} caches)",
            ev.databases.unwrap_or(0)
        )),
        "processing" => Some(format!(
            "shaders {:.1}% ({}/{})",
            ev.percent.unwrap_or(0.0),
            ev.done.unwrap_or(0),
            ev.total.unwrap_or(0)
        )),
        "finished" => Some("shaders: finishing".to_string()),
        "skipped" => Some(format!(
            "Shaders skipped: {}",
            ev.reason.unwrap_or_default()
        )),
        _ => None,
    }
}

/// Final status from shaders result.
pub fn shader_result_status(stdout: &str, success: bool) -> String {
    let last = stdout.lines().rev().find(|l| !l.trim().is_empty());
    let Some(ev) = last.and_then(|l| serde_json::from_str::<EventJson>(l.trim()).ok()) else {
        return if success {
            "Shaders processed".to_string()
        } else {
            "Shaders failed".to_string()
        };
    };
    if let Some(err) = ev.error_text() {
        return format!("Shaders failed: {err}");
    }
    match ev.status.as_deref() {
        Some("skipped") => format!("Shaders skipped: {}", ev.reason.unwrap_or_default()),
        _ => format!(
            "Shaders processed ({}/{})",
            ev.done.unwrap_or(0),
            ev.total.unwrap_or(0)
        ),
    }
}

/// Status for anti-cheat progress.
pub fn anticheat_event_status(kind: AntiCheat, line: &str) -> Option<String> {
    let ev: EventJson = serde_json::from_str(line.trim()).ok()?;
    if ev.event.as_deref() != Some("progress") {
        return None;
    }
    let state = ev.state.unwrap_or_else(|| "downloading".to_string());
    Some(format!(
        "{} {state} {:.1}%",
        kind.label(),
        ev.percent.unwrap_or(0.0)
    ))
}

/// Final status from anti-cheat result.
pub fn anticheat_result_status(kind: AntiCheat, stdout: &str, success: bool) -> String {
    // Pretty JSON spans many lines.
    let trimmed = stdout.trim();
    let parsed = serde_json::from_str::<EventJson>(trimmed).ok().or_else(|| {
        trimmed
            .lines()
            .rev()
            .find_map(|l| serde_json::from_str::<EventJson>(l.trim()).ok())
    });
    let Some(ev) = parsed else {
        return if success {
            format!("{} installed", kind.label())
        } else {
            format!("{} install failed", kind.label())
        };
    };
    if let Some(err) = ev.error_text() {
        return format!("{} failed: {err}", kind.label());
    }
    match ev.status.as_deref() {
        Some("already_installed") => format!("{} already installed", kind.label()),
        _ => format!("{} installed", kind.label()),
    }
}

fn set_status(status: &Arc<Mutex<Option<GameStatus>>>, msg: &str) {
    if let Ok(mut guard) = status.lock() {
        let next = GameStatus::msg(&guard, msg);
        *guard = Some(next);
    }
}

/// Run aurelia, streaming stderr statuses.
fn stream(
    args: Vec<String>,
    status: Arc<Mutex<Option<GameStatus>>>,
    on_line: impl Fn(&str) -> Option<String>,
    finish: impl Fn(&str, bool) -> String,
) {
    let spawned = process::Command::new(bin())
        .args(&args)
        .arg("--json")
        .stdin(process::Stdio::null())
        .stdout(process::Stdio::piped())
        .stderr(process::Stdio::piped())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(err) => {
            set_status(&status, &format!("Failed: {err}"));
            return;
        }
    };
    // Drain stdout so the child never blocks.
    let stdout = child.stdout.take();
    let result = thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut out) = stdout {
            let _ = out.read_to_string(&mut buf);
        }
        buf
    });
    if let Some(stderr) = child.stderr.take() {
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let label = crate::interface::aurelia_log::label(&refs);
        for line in crate::interface::aurelia_log::tee(&label, stderr) {
            if let Some(msg) = on_line(&line) {
                set_status(&status, &msg);
            }
        }
    }
    let out = result.join().unwrap_or_default();
    let ok = child.wait().map(|s| s.success()).unwrap_or(false);
    set_status(&status, &finish(&out, ok));
}

/// Run `aurelia shaders` on a thread.
pub fn spawn_shaders(app_id: i32, force: bool, status: Arc<Mutex<Option<GameStatus>>>) {
    set_status(&status, "shaders: starting");
    thread::spawn(move || {
        let mut args = vec!["shaders".to_string(), app_id.to_string()];
        if force {
            args.push("--force".to_string());
        }
        stream(args, status, shader_event_status, shader_result_status);
    });
}

/// Install an anti-cheat runtime, threaded.
pub fn spawn_anticheat_install(kind: AntiCheat, status: Arc<Mutex<Option<GameStatus>>>) {
    set_status(&status, &format!("{}: starting", kind.label()));
    thread::spawn(move || {
        let args = vec![
            "proton".to_string(),
            "anticheat".to_string(),
            kind.arg().to_string(),
        ];
        stream(
            args,
            status,
            move |l| anticheat_event_status(kind, l),
            move |out, ok| anticheat_result_status(kind, out, ok),
        );
    });
}
