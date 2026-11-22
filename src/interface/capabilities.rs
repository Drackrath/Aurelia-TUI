//! Probed `aurelia capabilities --json` document.

use std::collections::BTreeMap;
use std::process;
use std::sync::RwLock;
use std::thread;

use serde::Deserialize;

use crate::interface::aurelia;

/// One command's args, flags, subcommands.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CommandCaps {
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub flags: Vec<String>,
    #[serde(default)]
    pub subcommands: BTreeMap<String, CommandCaps>,
}

/// The `aurelia capabilities --json` document.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Capabilities {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub schema: u32,
    #[serde(default)]
    pub os: String,
    #[serde(default)]
    pub arch: String,
    #[serde(default)]
    pub global_flags: Vec<String>,
    #[serde(default)]
    pub commands: BTreeMap<String, CommandCaps>,
    #[serde(default)]
    pub events: Vec<String>,
    #[serde(default)]
    pub features: Vec<String>,
}

impl Capabilities {
    /// Parse the JSON document.
    pub fn parse(json: &str) -> Result<Capabilities, serde_json::Error> {
        serde_json::from_str(json.trim())
    }

    /// Feature, `cmd sub --flag`, or `event:x`.
    pub fn has(&self, feature: &str) -> bool {
        let feature = feature.trim();
        if feature.is_empty() {
            return false;
        }
        if let Some(event) = feature.strip_prefix("event:") {
            return self.events.iter().any(|e| e == event);
        }
        if self.features.iter().any(|f| f == feature) {
            return true;
        }
        let mut words = feature.split_whitespace().peekable();
        let Some(first) = words.next() else {
            return false;
        };
        let Some(mut cmd) = self.commands.get(first) else {
            return false;
        };
        while let Some(word) = words.peek() {
            if word.starts_with("--") {
                break;
            }
            match cmd.subcommands.get(*word) {
                Some(sub) => cmd = sub,
                None => return false,
            }
            words.next();
        }
        words.all(|flag| {
            cmd.flags.iter().any(|f| f == flag) || self.global_flags.iter().any(|f| f == flag)
        })
    }

    /// The [`MUST_HAVE`] entries this build lacks.
    pub fn missing(&self) -> Vec<&'static str> {
        MUST_HAVE.iter().copied().filter(|f| !self.has(f)).collect()
    }
}

/// Features the TUI cannot work without.
pub const MUST_HAVE: &[&str] = &[
    "login --health",
    "login --qr",
    "login.qr.result_line",
    "login.qr.stdin_control",
    "list",
    "account",
    "logout",
    "daemon stop",
];

/// The latest probe result.
#[derive(Default)]
struct Probe {
    /// Bumped per probe; drops stale results.
    generation: u64,
    version: Option<String>,
    caps: Option<Capabilities>,
}

static PROBE: RwLock<Probe> = RwLock::new(Probe {
    generation: 0,
    version: None,
    caps: None,
});

/// Version inline; capabilities in background.
pub fn probe() {
    let version = aurelia::version();
    let generation = {
        let Ok(mut probe) = PROBE.write() else {
            return;
        };
        probe.generation += 1;
        probe.version = version;
        probe.caps = None;
        probe.generation
    };
    thread::spawn(move || {
        let caps = fetch();
        if let Ok(mut probe) = PROBE.write()
            && probe.generation == generation
        {
            probe.caps = caps;
        }
    });
}

/// Run `aurelia capabilities --json`.
fn fetch() -> Option<Capabilities> {
    let output = process::Command::new(aurelia::bin())
        .args(["capabilities", "--json"])
        .stdin(process::Stdio::null())
        .stderr(process::Stdio::null())
        .output()
        .ok()?;
    Capabilities::parse(&String::from_utf8_lossy(&output.stdout)).ok()
}

/// The probed `aurelia --version`, if any.
pub fn version() -> Option<String> {
    PROBE.read().ok()?.version.clone()
}

/// A copy of the probed document, once loaded.
pub fn current() -> Option<Capabilities> {
    PROBE.read().ok()?.caps.clone()
}

/// Whether the installed build supports `feature`.
pub fn has(feature: &str) -> bool {
    PROBE
        .read()
        .ok()
        .and_then(|p| p.caps.as_ref().map(|c| c.has(feature)))
        .unwrap_or(false)
}

/// Missing must-haves; empty until probed.
pub fn missing() -> Vec<&'static str> {
    PROBE
        .read()
        .ok()
        .and_then(|p| p.caps.as_ref().map(Capabilities::missing))
        .unwrap_or_default()
}

/// Install a document directly (tests only).
#[doc(hidden)]
pub fn set_for_test(version: Option<&str>, caps: Option<Capabilities>) {
    if let Ok(mut probe) = PROBE.write() {
        probe.generation += 1;
        probe.version = version.map(str::to_string);
        probe.caps = caps;
    }
}
