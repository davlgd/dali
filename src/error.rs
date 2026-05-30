//! Error type shared across DALI.
//!
//! A single [`Error`] enum keeps error handling uniform (DRY) while staying
//! descriptive enough to surface actionable messages to the user.

use std::path::PathBuf;
use std::process::ExitStatus;

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, Error>;

/// Every failure DALI can produce.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A pre-flight environment check failed (not UEFI, no network, not root…).
    #[error("environment check failed: {0}")]
    Environment(String),

    /// The supplied or gathered configuration is invalid.
    #[error("invalid configuration: {0}")]
    Config(String),

    /// An external command could not be spawned.
    #[error("failed to launch `{command}`: {source}")]
    Spawn {
        /// The command that could not be launched.
        command: String,
        /// The underlying OS error.
        source: std::io::Error,
    },

    /// An external command ran but exited with a non-zero status.
    #[error("command `{command}` exited with {status}\n{stderr}")]
    Command {
        /// The command that failed.
        command: String,
        /// The reported exit status.
        status: ExitStatus,
        /// Captured standard error, for diagnostics.
        stderr: String,
    },

    /// A filesystem operation failed.
    #[error("I/O error on {path}: {source}")]
    Io {
        /// The path involved in the failed operation.
        path: PathBuf,
        /// The underlying OS error.
        source: std::io::Error,
    },

    /// (De)serialization of a configuration file failed.
    #[error("could not parse configuration: {0}")]
    Serde(#[from] serde_json::Error),

    /// The terminal user interface failed.
    #[error("terminal interface error: {0}")]
    Tui(String),

    /// The user aborted the installation.
    #[error("installation aborted by user")]
    Aborted,
}

impl Error {
    /// Helper to build an [`Error::Io`] from a path and source error.
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }
}
