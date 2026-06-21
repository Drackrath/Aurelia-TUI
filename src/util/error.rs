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
