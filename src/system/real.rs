//! The real effects implementation, used on the live ISO.

use std::io::Write;
use std::path::Path;
use std::process::{Command as OsCommand, Stdio};

use super::{Command, Sys};
use crate::error::{Error, Result};

/// Executes commands and writes files for real.
#[derive(Debug, Default, Clone, Copy)]
pub struct RealSys;

impl RealSys {
    fn build(command: &Command) -> OsCommand {
        let mut os = OsCommand::new(&command.program);
        os.args(&command.args);
        os
    }

    fn spawn_error(command: &Command, source: std::io::Error) -> Error {
        Error::Spawn {
            command: command.to_string(),
            source,
        }
    }
}

impl Sys for RealSys {
    fn run(&self, command: &Command) -> Result<()> {
        let mut os = Self::build(command);
        if command.stdin.is_some() {
            os.stdin(Stdio::piped());
        }
        os.stderr(Stdio::piped());

        let mut child = os.spawn().map_err(|e| Self::spawn_error(command, e))?;
        if let Some(data) = &command.stdin
            && let Some(mut sink) = child.stdin.take()
        {
            sink.write_all(data.as_bytes())
                .map_err(|e| Self::spawn_error(command, e))?;
        }
        let output = child
            .wait_with_output()
            .map_err(|e| Self::spawn_error(command, e))?;

        if output.status.success() {
            Ok(())
        } else {
            Err(Error::Command {
                command: command.to_string(),
                status: output.status,
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            })
        }
    }

    fn capture(&self, command: &Command) -> Result<String> {
        let mut os = Self::build(command);
        // Both stdout (the value we want) and stderr must be piped so
        // `wait_with_output` actually collects them; an un-piped stdout is
        // inherited and `output.stdout` comes back empty.
        os.stdout(Stdio::piped());
        os.stderr(Stdio::piped());
        if command.stdin.is_some() {
            os.stdin(Stdio::piped());
        }
        let mut child = os.spawn().map_err(|e| Self::spawn_error(command, e))?;
        if let Some(data) = &command.stdin
            && let Some(mut sink) = child.stdin.take()
        {
            sink.write_all(data.as_bytes())
                .map_err(|e| Self::spawn_error(command, e))?;
        }
        let output = child
            .wait_with_output()
            .map_err(|e| Self::spawn_error(command, e))?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            Err(Error::Command {
                command: command.to_string(),
                status: output.status,
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            })
        }
    }

    fn write(&self, path: &str, contents: &str) -> Result<()> {
        if let Some(parent) = Path::new(path).parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }
        std::fs::write(path, contents).map_err(|e| Error::io(path, e))
    }

    fn mkdir_p(&self, path: &str) -> Result<()> {
        std::fs::create_dir_all(path).map_err(|e| Error::io(path, e))
    }

    fn append(&self, path: &str, contents: &str) -> Result<()> {
        use std::io::Write as _;
        if let Some(parent) = Path::new(path).parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| Error::io(path, e))?;
        file.write_all(contents.as_bytes())
            .map_err(|e| Error::io(path, e))
    }

    fn is_real(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::Command;

    #[test]
    fn capture_returns_command_stdout() {
        // Regression guard: capture must pipe (not inherit) stdout, otherwise
        // it silently returns an empty string — which once produced an
        // unbootable install (empty root UUID / fstab).
        let out = RealSys.capture(&Command::new("echo").arg("hello")).unwrap();
        assert_eq!(out.trim(), "hello");
    }

    #[test]
    fn capture_can_feed_stdin() {
        let out = RealSys
            .capture(&Command::new("cat").stdin("piped-in"))
            .unwrap();
        assert_eq!(out, "piped-in");
    }

    #[test]
    fn run_errors_on_nonzero_exit() {
        assert!(RealSys.run(&Command::new("false")).is_err());
    }
}
