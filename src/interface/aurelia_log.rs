//! Aurelia stderr log in the cache dir.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{ChildStderr, Command, Output, Stdio};
use std::sync::{Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::util::paths::cache_directory;

/// Log file name in the cache dir.
pub const LOG_FILE: &str = "aurelia.log";
/// Rotate past this size.
pub const MAX_BYTES: u64 = 1024 * 1024;

/// Append-only log with one backup.
pub struct LogSink {
    path: PathBuf,
    max_bytes: u64,
    file: Option<File>,
    size: u64,
}

impl LogSink {
    pub fn new(path: PathBuf, max_bytes: u64) -> LogSink {
        LogSink {
            path,
            max_bytes,
            file: None,
            size: 0,
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The rotated backup path.
    pub fn backup_path(&self) -> PathBuf {
        backup_of(&self.path)
    }

    /// Write one stamped, labelled line.
    pub fn append(&mut self, label: &str, line: &str) {
        let text = format!("{} [{}] {}\n", timestamp(now_secs()), label, line);
        if self.file.is_none() {
            self.open();
        }
        if self.size + text.len() as u64 > self.max_bytes && self.size > 0 {
            self.rotate();
        }
        if let Some(file) = self.file.as_mut()
            && file.write_all(text.as_bytes()).is_ok()
        {
            self.size += text.len() as u64;
        }
    }

    fn open(&mut self) {
        if let Some(dir) = self.path.parent() {
            let _ = fs::create_dir_all(dir);
        }
        let mut options = OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            // Owner-only, may hold paths.
            options.mode(0o600);
        }
        self.file = options.open(&self.path).ok();
        self.size = fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0);
    }

    fn rotate(&mut self) {
        self.file = None;
        let _ = fs::rename(&self.path, self.backup_path());
        self.open();
    }
}

fn backup_of(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".1");
    PathBuf::from(name)
}

/// The default log path.
pub fn log_path() -> Option<PathBuf> {
    cache_directory().ok().map(|dir| dir.join(LOG_FILE))
}

fn sink() -> &'static Mutex<Option<LogSink>> {
    static SINK: OnceLock<Mutex<Option<LogSink>>> = OnceLock::new();
    SINK.get_or_init(|| Mutex::new(log_path().map(|p| LogSink::new(p, MAX_BYTES))))
}

/// Append one line to the log.
pub fn append(label: &str, line: &str) {
    if let Ok(mut guard) = sink().lock()
        && let Some(sink) = guard.as_mut()
    {
        sink.append(label, line);
    }
}

/// Drops blank lines and progress ticks.
pub fn keep_line(line: &str) -> bool {
    let line = line.trim();
    if line.is_empty() {
        return false;
    }
    if !line.starts_with('{') {
        return true;
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
        return true;
    };
    let event = value.get("event").and_then(|e| e.as_str());
    let state = value.get("state").and_then(|s| s.as_str());
    !matches!(
        (event, state),
        (Some("progress"), _) | (Some("shaders"), Some("processing"))
    )
}

/// Log captured stderr bytes.
pub fn record(label: &str, stderr: &[u8]) {
    for line in String::from_utf8_lossy(stderr).lines() {
        if keep_line(line) {
            append(label, line.trim_end());
        }
    }
}

/// Lines of `reader`, logged as read.
pub fn tee<R: Read>(label: &str, reader: R) -> impl Iterator<Item = String> {
    let label = label.to_string();
    BufReader::new(reader)
        .lines()
        .map_while(Result::ok)
        .inspect(move |line| {
            if keep_line(line) {
                append(&label, line.trim_end());
            }
        })
}

/// Drain `stderr` into the log.
pub fn pump(label: &str, stderr: Option<ChildStderr>) -> Option<JoinHandle<()>> {
    let stderr = stderr?;
    let label = label.to_string();
    Some(thread::spawn(move || for _ in tee(&label, stderr) {}))
}

/// Run to completion, logging stderr live.
pub fn output(command: &mut Command, label: &str) -> io::Result<Output> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let pumping = pump(label, child.stderr.take());
    let output = child.wait_with_output();
    if let Some(handle) = pumping {
        let _ = handle.join();
    }
    output
}

/// Log label: args before flags.
pub fn label(args: &[&str]) -> String {
    args.iter()
        .take_while(|a| !a.starts_with('-'))
        .take(2)
        .copied()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Backup then current log lines.
pub fn read_lines_from(path: &Path) -> Vec<String> {
    let mut lines = Vec::new();
    for file in [backup_of(path), path.to_path_buf()] {
        if let Ok(text) = fs::read_to_string(&file) {
            lines.extend(text.lines().map(str::to_string));
        }
    }
    lines
}

/// Lines of the default log.
pub fn read_lines() -> Vec<String> {
    log_path().map(|p| read_lines_from(&p)).unwrap_or_default()
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// `YYYY-MM-DD HH:MM:SS` in UTC.
pub fn timestamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// Days since epoch to date.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    // Howard Hinnant's civil_from_days.
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}
