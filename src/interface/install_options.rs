//! Install platform, language and set-language.

use std::io::{BufRead, BufReader, Read};
use std::process;
use std::sync::{Arc, Mutex};
use std::thread;

use serde::Deserialize;

use crate::interface::aurelia::{ProgressJson, bin, eta_suffix, json_error, run_json, set_status};
use crate::interface::game_status::GameStatus;
use crate::util::error::STError;
use crate::util::log::log;

/// The `install --dry-run --json` object.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct DryRunJson {
    #[serde(default)]
    pub app_id: u32,
    /// The platform sized (auto resolves to one).
    #[serde(default)]
    pub platform: String,
    #[serde(default)]
    pub download_size: u64,
    #[serde(default)]
    pub disk_size: u64,
    #[serde(default)]
    pub depot_count: u32,
    #[serde(default)]
    pub languages: Vec<String>,
    #[serde(default)]
    pub default_language: Option<String>,
    #[serde(default)]
    pub installed_language: Option<String>,
}

impl DryRunJson {
    /// Whether the platform has any depots.
    pub fn has_build(&self) -> bool {
        self.depot_count > 0
    }

    /// Whether a language choice is worth showing.
    pub fn offers_languages(&self) -> bool {
        self.languages.len() > 1
    }
}

/// Parse a dry-run JSON value.
pub fn parse_dry_run(value: serde_json::Value) -> Result<DryRunJson, STError> {
    Ok(serde_json::from_value(value)?)
}

/// Size a build (`install <id> --dry-run [-p <platform>] --json`).
pub fn dry_run(app_id: i32, platform: Option<&str>) -> Result<DryRunJson, STError> {
    let id = app_id.to_string();
    let mut args = vec!["install", id.as_str(), "--dry-run"];
    if let Some(p) = platform {
        args.push("-p");
        args.push(p);
    }
    parse_dry_run(run_json(&args)?)
}

/// The build platform the install picker offers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum PlatformChoice {
    /// Let Aurelia pick (Windows first).
    #[default]
    Auto,
    Windows,
    Linux,
}

impl PlatformChoice {
    pub const ALL: [PlatformChoice; 3] = [Self::Auto, Self::Windows, Self::Linux];

    /// The `-p` value, `None` for auto.
    pub fn arg(self) -> Option<&'static str> {
        match self {
            Self::Auto => None,
            Self::Windows => Some("windows"),
            Self::Linux => Some("linux"),
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Windows => "windows",
            Self::Linux => "linux",
        }
    }

    /// The next choice, wrapping around.
    pub fn next(self) -> Self {
        match self {
            Self::Auto => Self::Windows,
            Self::Windows => Self::Linux,
            Self::Linux => Self::Auto,
        }
    }

    /// Parse an installed platform string.
    pub fn from_installed(platform: Option<&str>) -> Self {
        match platform.map(str::to_ascii_lowercase).as_deref() {
            Some("windows") => Self::Windows,
            Some("linux") => Self::Linux,
            _ => Self::Auto,
        }
    }
}

/// Extra flags an install runs with.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InstallOpts {
    pub platform: Option<String>,
    pub language: Option<String>,
}

impl InstallOpts {
    /// The CLI flags, e.g. `-p linux`.
    pub fn args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if let Some(p) = &self.platform {
            args.push("-p".to_string());
            args.push(p.clone());
        }
        if let Some(l) = &self.language {
            args.push("--language".to_string());
            args.push(l.clone());
        }
        args
    }
}

/// A readable name for a Steam language.
pub fn language_label(name: &str) -> String {
    let special = match name {
        "schinese" => Some("Simplified Chinese"),
        "tchinese" => Some("Traditional Chinese"),
        "koreana" => Some("Korean"),
        "brazilian" => Some("Portuguese (Brazil)"),
        "latam" => Some("Spanish (Latin America)"),
        "spanish" => Some("Spanish (Spain)"),
        _ => None,
    };
    if let Some(label) = special {
        return label.to_string();
    }
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// Worker for `set-language <id> <lang> --json`.
pub fn set_language(id: i32, language: String, status: Arc<Mutex<Option<GameStatus>>>) {
    set_status(&status, "processing...");

    let id_str = id.to_string();
    let spawned = process::Command::new(bin())
        .args(["set-language", &id_str, &language, "--json"])
        .stdin(process::Stdio::null())
        .stdout(process::Stdio::piped())
        .stderr(process::Stdio::piped())
        .spawn();

    let mut child = match spawned {
        Ok(child) => child,
        Err(err) => {
            set_status(&status, &format!("Failed: {}", err));
            log!("Failed to spawn aurelia set-language", id, err);
            return;
        }
    };

    // Drain stdout off-thread.
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
            if let Some(msg) = progress_line(line.trim()) {
                set_status(&status, &msg);
            }
        }
    }

    let result = result_handle.join().unwrap_or_default();
    let _ = child.wait();
    set_status(&status, &finish_message(result.trim()));
}

/// Status text for a progress line.
pub fn progress_line(line: &str) -> Option<String> {
    let event = serde_json::from_str::<ProgressJson>(line).ok()?;
    if event.event.as_deref() != Some("progress") {
        return None;
    }
    Some(format!(
        "changing language {:.1}%{}",
        event.percent.unwrap_or(0.0),
        eta_suffix(event.eta_seconds)
    ))
}

/// Final status from the stdout result.
pub fn finish_message(stdout: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(stdout) {
        Ok(value) => {
            if let Some(err) = json_error(&value) {
                format!("Failed: {}", err)
            } else if value.get("status").and_then(|s| s.as_str()) == Some("language_set") {
                let lang = value
                    .get("language")
                    .and_then(|l| l.as_str())
                    .map(language_label)
                    .unwrap_or_default();
                format!("language set: {lang}")
            } else {
                "done".to_string()
            }
        }
        Err(_) => "done".to_string(),
    }
}

#[cfg(test)]
#[path = "install_options_tests.rs"]
mod tests;
