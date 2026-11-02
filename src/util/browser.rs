//! Open a URL using the platform's default browser. Call from a worker: platform
//! launchers can block while locating or starting the browser.
use crate::util::error::STError;
use std::process::{Command, Stdio};

pub fn open(url: &str) -> Result<(), STError> {
    #[cfg(target_os = "macos")]
    let mut command = Command::new("open");
    #[cfg(windows)]
    let mut command = {
        let mut command = Command::new("cmd");
        command.args(["/c", "start", ""]);
        command
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = Command::new("xdg-open");
    let status = command
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()?;
    if !status.success() {
        return Err(STError::Problem(format!(
            "Browser launcher exited with {status}"
        )));
    }
    Ok(())
}
