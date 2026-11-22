//! Parsing aurelia's `--json` stdout.

use std::process::Output;

use serde_json::Value;

use crate::util::error::{CliError, STError};

/// Every JSON value on stdout.
pub fn values(stdout: &str) -> Result<Vec<Value>, serde_json::Error> {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    if let Ok(value) = serde_json::from_str(trimmed) {
        return Ok(vec![value]);
    }
    serde_json::Deserializer::from_str(trimmed)
        .into_iter::<Value>()
        .collect()
}

/// The deciding value: last error, else last.
pub fn terminal(values: Vec<Value>) -> Option<Value> {
    let last_error = values
        .iter()
        .rposition(|v| CliError::from_json(v).is_some());
    match last_error {
        Some(i) => values.into_iter().nth(i),
        None => values.into_iter().last(),
    }
}

/// Value, typed error, or `None`.
pub fn parse(stdout: &str, exit_code: Option<i32>) -> Result<Option<Value>, STError> {
    let Some(value) = terminal(values(stdout)?) else {
        return Ok(None);
    };
    if let Some(err) = CliError::from_json(&value) {
        return Err(STError::Cli(err.with_exit_code(exit_code)));
    }
    Ok(Some(value))
}

/// Last non-empty stderr line.
pub fn last_stderr_line(stderr: &[u8]) -> Option<String> {
    String::from_utf8_lossy(stderr)
        .lines()
        .map(str::trim)
        .rfind(|l| !l.is_empty())
        .map(str::to_string)
}

/// Finish a `run_json` style call.
pub fn finish(args: &[&str], output: &Output, empty_hint: &str) -> Result<Value, STError> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    match parse(&stdout, output.status.code())? {
        Some(value) => Ok(value),
        None => {
            let mut msg = format!("aurelia produced no output for `{}`", args.join(" "));
            if !empty_hint.is_empty() {
                msg.push_str(&format!(" ({empty_hint})"));
            }
            if let Some(line) = last_stderr_line(&output.stderr) {
                msg.push_str(&format!(": {line}"));
            }
            Err(STError::Problem(msg))
        }
    }
}
