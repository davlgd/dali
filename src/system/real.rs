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

    /// Spawn `command`, feed any `stdin` payload, wait, and map a non-zero exit
    /// to [`Error::Command`]. `capture_stdout` pipes stdout (so the caller can
    /// read it); without it stdout is inherited. stderr is always piped for the
    /// error message.
    fn exec(command: &Command, capture_stdout: bool) -> Result<std::process::Output> {
        let mut os = Self::build(command);
        if capture_stdout {
            os.stdout(Stdio::piped());
        }
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
            Ok(output)
        } else {
            Err(Error::Command {
                command: command.to_string(),
                status: output.status,
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            })
        }
    }
}

impl Sys for RealSys {
    fn run(&self, command: &Command) -> Result<()> {
        Self::exec(command, false).map(|_| ())
    }

    fn capture(&self, command: &Command) -> Result<String> {
        // stdout must be piped (not inherited) or `output.stdout` comes back
        // empty — which once produced an unbootable install (empty root UUID).
        Self::exec(command, true).map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
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

    fn write_block(&self, path: &str, begin: &str, end: &str, block: &str) -> Result<()> {
        let existing = std::fs::read_to_string(path).unwrap_or_default();

        // Keep a one-time backup before the first modification.
        let backup = format!("{path}.dali.bak");
        if !existing.is_empty() && !Path::new(&backup).exists() {
            std::fs::write(&backup, &existing).map_err(|e| Error::io(&backup, e))?;
        }

        self.write(path, &splice_block(&existing, begin, end, block))
    }

    fn is_real(&self) -> bool {
        true
    }
}

/// Replace the `begin`..`end` region of `existing` with `block`, or append
/// `block` when no such region exists. `block` carries its own markers and
/// leading newline, so a fresh append matches a plain append.
fn splice_block(existing: &str, begin: &str, end: &str, block: &str) -> String {
    if let Some(b) = existing.find(begin)
        && let Some(rel) = existing[b..].find(end)
    {
        let marker_end = b + rel + end.len();
        // Extend to the end of the end-marker's line (consume its newline).
        let line_end = existing[marker_end..]
            .find('\n')
            .map_or(existing.len(), |n| marker_end + n + 1);
        let before = existing[..b].trim_end_matches('\n');
        let after = &existing[line_end..];

        let mut out = String::from(before);
        out.push_str(block); // block begins with its own newline + marker
        if !after.is_empty() {
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(after);
        }
        return out;
    }
    format!("{existing}{block}")
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

    #[test]
    fn write_block_is_idempotent_and_preserves_user_content() {
        let dir = std::env::temp_dir().join(format!("dali-wb-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bashrc").to_string_lossy().into_owned();
        std::fs::write(&path, "export X=1\n").unwrap();
        let (begin, end) = ("# >>> B >>>", "# <<< B <<<");

        RealSys
            .write_block(&path, begin, end, "\n# >>> B >>>\nalias a=1\n# <<< B <<<\n")
            .unwrap();
        let after1 = std::fs::read_to_string(&path).unwrap();
        assert!(after1.contains("export X=1"));
        assert_eq!(after1.matches(begin).count(), 1);

        // Re-run with a changed block: still exactly one marker, user line kept.
        RealSys
            .write_block(&path, begin, end, "\n# >>> B >>>\nalias a=2\n# <<< B <<<\n")
            .unwrap();
        let after2 = std::fs::read_to_string(&path).unwrap();
        assert_eq!(after2.matches(begin).count(), 1, "block not duplicated");
        assert!(after2.contains("alias a=2") && !after2.contains("alias a=1"));
        assert!(after2.contains("export X=1"));
        assert!(Path::new(&format!("{path}.dali.bak")).exists());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
