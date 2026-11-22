//! Depot manifest browsing and downloads.

use serde::Deserialize;

use crate::interface::steam_restart::run;
use crate::util::error::STError;

/// One `files` array entry.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DepotFileJson {
    /// Path inside the depot, `/`-separated.
    #[serde(default)]
    pub path: String,
    /// `file`, `directory` or `symlink`.
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub executable: bool,
    #[serde(default)]
    pub link_target: Option<String>,
    /// Content SHA-1 hex; empty for directories.
    #[serde(default)]
    pub sha1: String,
    #[serde(default)]
    pub chunks: u64,
}

impl DepotFileJson {
    pub fn is_file(&self) -> bool {
        self.kind == "file"
    }

    pub fn is_dir(&self) -> bool {
        self.kind == "directory"
    }
}

/// `depots files --json` payload.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DepotFilesJson {
    #[serde(default)]
    pub app_id: u32,
    #[serde(default)]
    pub depot_id: u32,
    #[serde(default)]
    pub manifest_id: u64,
    #[serde(default)]
    pub total_size: u64,
    #[serde(default)]
    pub files: Vec<DepotFileJson>,
}

/// `depots download --json` payload.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DepotDownloadJson {
    #[serde(default)]
    pub app_id: u32,
    #[serde(default)]
    pub depot_id: u32,
    #[serde(default)]
    pub manifest_id: u64,
    /// Saved file, absolute.
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub size: u64,
}

/// Which manifest to read.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ManifestSel {
    /// Public branch's current manifest.
    #[default]
    Current,
    Branch(String),
    Manifest(u64),
}

impl ManifestSel {
    /// Digits: manifest id; else branch.
    pub fn parse(text: &str) -> ManifestSel {
        let text = text.trim();
        if text.is_empty() || text == "public" {
            ManifestSel::Current
        } else if let Ok(id) = text.parse::<u64>() {
            ManifestSel::Manifest(id)
        } else {
            ManifestSel::Branch(text.to_string())
        }
    }

    /// Text for the input field.
    pub fn label(&self) -> String {
        match self {
            ManifestSel::Current => String::new(),
            ManifestSel::Branch(b) => b.clone(),
            ManifestSel::Manifest(m) => m.to_string(),
        }
    }

    fn push_args(&self, args: &mut Vec<String>) {
        match self {
            ManifestSel::Current => {}
            ManifestSel::Branch(b) => {
                args.push("--branch".to_string());
                args.push(b.clone());
            }
            ManifestSel::Manifest(m) => {
                args.push("--manifest".to_string());
                args.push(m.to_string());
            }
        }
    }
}

/// Arguments for `depots files`.
pub fn files_args(app_id: u32, depot_id: u32, sel: &ManifestSel, filter: &str) -> Vec<String> {
    let mut args = vec![
        "depots".to_string(),
        "files".to_string(),
        app_id.to_string(),
        depot_id.to_string(),
    ];
    sel.push_args(&mut args);
    if !filter.trim().is_empty() {
        args.push("--filter".to_string());
        args.push(filter.trim().to_string());
    }
    args
}

/// Arguments for `depots download`.
pub fn download_args(
    app_id: u32,
    depot_id: u32,
    path: &str,
    sel: &ManifestSel,
    out_dir: &str,
    force: bool,
) -> Vec<String> {
    let mut args = vec![
        "depots".to_string(),
        "download".to_string(),
        app_id.to_string(),
        depot_id.to_string(),
        path.to_string(),
    ];
    sel.push_args(&mut args);
    args.push("--output".to_string());
    args.push(out_dir.to_string());
    if force {
        args.push("--force".to_string());
    }
    args
}

fn run_owned(args: &[String]) -> Result<serde_json::Value, STError> {
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run(&refs)
}

/// List a depot manifest's entries.
pub fn files(
    app_id: u32,
    depot_id: u32,
    sel: &ManifestSel,
    filter: &str,
) -> Result<DepotFilesJson, STError> {
    let value = run_owned(&files_args(app_id, depot_id, sel, filter))?;
    Ok(serde_json::from_value(value)?)
}

/// Download one manifest file into `out_dir`.
pub fn download(
    app_id: u32,
    depot_id: u32,
    path: &str,
    sel: &ManifestSel,
    out_dir: &str,
    force: bool,
) -> Result<DepotDownloadJson, STError> {
    let value = run_owned(&download_args(app_id, depot_id, path, sel, out_dir, force))?;
    Ok(serde_json::from_value(value)?)
}

/// Expand `~` and env vars.
pub fn expand_dir(dir: &str) -> String {
    shellexpand::full(dir.trim())
        .map(|s| s.into_owned())
        .unwrap_or_else(|_| dir.trim().to_string())
}

/// Default download folder.
pub fn default_download_dir() -> String {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();
    if home.is_empty() {
        return ".".to_string();
    }
    let downloads = std::path::Path::new(&home).join("Downloads");
    if downloads.is_dir() {
        "~/Downloads".to_string()
    } else {
        "~".to_string()
    }
}

/// Outcome of a download, for display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadOutcome {
    /// Saved; Aurelia checked the SHA-1.
    Verified {
        path: String,
        sha1: String,
    },
    /// Saved; manifest had no checksum.
    Unchecked {
        path: String,
    },
    /// Target exists; retry needs `--force`.
    Exists(String),
    /// SHA-1 failed on every CDN pool.
    ChecksumFailed(String),
    Failed(String),
}

/// Success means Aurelia's SHA-1 check passed.
pub fn outcome(
    entry_sha1: &str,
    size: u64,
    result: Result<DepotDownloadJson, STError>,
) -> DownloadOutcome {
    match result {
        Ok(done) if !entry_sha1.is_empty() && size > 0 => DownloadOutcome::Verified {
            path: done.path,
            sha1: entry_sha1.to_string(),
        },
        Ok(done) => DownloadOutcome::Unchecked { path: done.path },
        Err(err) => {
            let msg = err.to_string();
            if msg.contains("failed its checksum") {
                DownloadOutcome::ChecksumFailed(msg)
            } else if msg.contains("already exists") && msg.contains("--force") {
                DownloadOutcome::Exists(msg)
            } else {
                DownloadOutcome::Failed(msg)
            }
        }
    }
}

impl DownloadOutcome {
    /// One status line.
    pub fn line(&self) -> String {
        match self {
            DownloadOutcome::Verified { path, sha1 } => {
                format!("Saved {path} \u{2014} SHA-1 OK ({sha1})")
            }
            DownloadOutcome::Unchecked { path } => {
                format!("Saved {path} \u{2014} no SHA-1 to check")
            }
            DownloadOutcome::Exists(msg) => format!("{msg}. Overwrite? (y/n)"),
            DownloadOutcome::ChecksumFailed(msg) => format!("SHA-1 mismatch: {msg}"),
            DownloadOutcome::Failed(msg) => format!("Failed: {msg}"),
        }
    }
}
