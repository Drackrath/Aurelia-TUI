use crate::client::Command;

use shellexpand::LookupError;
use std::error;
use std::fmt;
use std::fmt::Debug;
use std::io;
use std::sync::mpsc::SendError;
use std::sync::PoisonError;

#[derive(Debug)]
pub enum STError {
    Io(io::Error),
    Process(io::Error),
    Problem(String),
    /// Typed `--json` error from aurelia.
    Cli(CliError),
}

/// Aurelia's `--json` error object.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CliError {
    /// Stable kind, e.g. `auth_required`.
    pub kind: Option<String>,
    pub message: String,
    pub hint: Option<String>,
    pub retry_after_seconds: Option<u64>,
    /// Process exit code, when known.
    pub exit_code: Option<i32>,
}

impl CliError {
    /// Parse a top-level error object.
    pub fn from_json(value: &serde_json::Value) -> Option<CliError> {
        let message = value.get("error").and_then(|e| e.as_str())?;
        let text = |key: &str| {
            value
                .get(key)
                .and_then(|v| v.as_str())
                .filter(|v| !v.is_empty())
                .map(str::to_string)
        };
        Some(CliError {
            kind: text("type"),
            message: message.to_string(),
            hint: text("hint"),
            retry_after_seconds: value.get("retry_after_seconds").and_then(|v| v.as_u64()),
            exit_code: None,
        })
    }

    /// The same error with an exit code.
    pub fn with_exit_code(mut self, code: Option<i32>) -> CliError {
        self.exit_code = code;
        self
    }

    /// Whether `kind` matches this error.
    pub fn is(&self, kind: &str) -> bool {
        self.kind.as_deref() == Some(kind)
    }

    /// "message — hint" for status lines.
    pub fn summary(&self) -> String {
        match self.hint.as_deref() {
            Some(hint) if !self.message.contains(hint) => {
                format!("{} — {}", self.message, hint)
            }
            _ => self.message.clone(),
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.summary())
    }
}

impl STError {
    /// Aurelia's error `type`, if typed.
    pub fn kind(&self) -> Option<&str> {
        self.cli().and_then(|e| e.kind.as_deref())
    }

    /// Aurelia's `hint`, if any.
    pub fn hint(&self) -> Option<&str> {
        self.cli().and_then(|e| e.hint.as_deref())
    }

    /// The typed CLI error, if any.
    pub fn cli(&self) -> Option<&CliError> {
        match self {
            STError::Cli(e) => Some(e),
            _ => None,
        }
    }

    /// Whether the error has this `type`.
    pub fn is_kind(&self, kind: &str) -> bool {
        self.kind() == Some(kind)
    }

    /// The message without the hint.
    pub fn message(&self) -> String {
        match self {
            STError::Cli(e) => e.message.clone(),
            STError::Problem(msg) => msg.clone(),
            other => other.to_string(),
        }
    }
}

impl From<CliError> for STError {
    fn from(err: CliError) -> STError {
        STError::Cli(err)
    }
}

impl From<io::Error> for STError {
    fn from(err: io::Error) -> STError {
        STError::Io(err)
    }
}

impl From<SendError<Command>> for STError {
    fn from(err: SendError<Command>) -> STError {
        STError::Problem(format!("{:?}", err))
    }
}

impl From<serde_json::Error> for STError {
    fn from(err: serde_json::Error) -> STError {
        STError::Problem(format!("{:?}", err))
    }
}

impl<T> From<PoisonError<T>> for STError {
    fn from(err: PoisonError<T>) -> STError {
        STError::Problem(format!("{:?}", err))
    }
}

impl<T: Debug> From<LookupError<T>> for STError {
    fn from(err: LookupError<T>) -> STError {
        STError::Problem(format!("{:?}", err))
    }
}

impl fmt::Display for STError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &*self {
            STError::Process(e) => write!(
                f,
                "An error occured spawning the aurelia process. Is the `aurelia` CLI installed and on your PATH?\n{:?}",
                e
            ),
            // The CLI's own message is already human-readable; show it directly
            // rather than the `Problem("…")` debug wrapper.
            STError::Problem(msg) => write!(f, "{}", msg),
            STError::Cli(err) => write!(f, "{}", err),
            _ => write!(f, "{:?}", self),
        }
    }
}

impl error::Error for STError {
    fn description(&self) -> &str {
        "woosp"
    }

    fn cause(&self) -> Option<&dyn error::Error> {
        // Pass on reference
        None
    }
}
