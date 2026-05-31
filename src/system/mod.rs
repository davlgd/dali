//! The effects boundary.
//!
//! Every side effect DALI performs — running a command, writing a file,
//! enabling a service — goes through the [`Sys`] trait. This is the single
//! seam that lets us (a) run for real on the live ISO, (b) print a plan in
//! `--dry-run`, and (c) test steps without touching the host. It is the
//! Dependency-Inversion "D" of SOLID applied to a tool that is otherwise all
//! side effects.

mod dry;
mod real;

pub mod probe;

pub use dry::DrySys;
pub use real::RealSys;

use std::fmt;

use crate::config::stack;
use crate::error::Result;

/// A command to execute: a program and its arguments, plus optional standard
/// input and a human-readable description for logs and dry-runs.
#[derive(Clone, Debug)]
pub struct Command {
    /// Program to run, e.g. `pacstrap`.
    pub program: String,
    /// Arguments passed to the program.
    pub args: Vec<String>,
    /// Optional data piped to the program's standard input.
    pub stdin: Option<String>,
}

impl Command {
    /// Start building a command from a program name.
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            stdin: None,
        }
    }

    /// Append a single argument.
    #[must_use]
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Append several arguments.
    #[must_use]
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Attach data to be written to the program's standard input.
    #[must_use]
    pub fn stdin(mut self, data: impl Into<String>) -> Self {
        self.stdin = Some(data.into());
        self
    }

    /// Wrap this command so it runs inside the target system via `arch-chroot`.
    #[must_use]
    pub fn in_chroot(self) -> Self {
        Command {
            program: "arch-chroot".to_owned(),
            args: std::iter::once(stack::TARGET_MOUNT.to_owned())
                .chain(std::iter::once(self.program))
                .chain(self.args)
                .collect(),
            stdin: self.stdin,
        }
    }
}

// The `Display` form is illustrative (used in dry-run output and error
// messages): whitespace args are quoted for readability, but it performs no
// shell escaping. Commands actually run via the OS without a shell, so this is
// not a correctness or security concern — do not treat the output as
// copy-paste-safe shell.
impl fmt::Display for Command {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.program)?;
        for arg in &self.args {
            if arg.chars().any(char::is_whitespace) {
                write!(f, " \"{arg}\"")?;
            } else {
                write!(f, " {arg}")?;
            }
        }
        Ok(())
    }
}

/// The set of side effects available to installation steps.
///
/// Implementors must honour the contract that read-only probing happens
/// elsewhere ([`probe`]); everything here is a mutation of either the host
/// process state or the target system.
pub trait Sys {
    /// Run a command, returning an error if it exits non-zero.
    ///
    /// Any `stdin` payload is assumed small: it is written in full before the
    /// child's output is drained, which is true for every current caller (short
    /// `chpasswd` lines). Piping more than a pipe buffer (~64 KiB) could
    /// deadlock and would need concurrent draining.
    fn run(&self, command: &Command) -> Result<()>;

    /// Run a command and capture its standard output as a string.
    ///
    /// The dry-run implementation returns an empty string (it performs
    /// nothing), so a step that branches on captured output must guard with
    /// [`Self::is_real`] — see `steps::bootloader` for the pattern.
    ///
    /// Like [`Self::run`], any `stdin` payload is assumed small: it is written
    /// in full before the child's output is drained, so piping more than a pipe
    /// buffer (~64 KiB) could deadlock.
    fn capture(&self, command: &Command) -> Result<String>;

    /// Write `contents` to `path` on the live system, replacing any existing
    /// file. `path` is taken as-is, so callers prefix `/mnt` themselves via
    /// [`target_path`].
    fn write(&self, path: &str, contents: &str) -> Result<()>;

    /// Create a directory and all missing parents on the live system.
    fn mkdir_p(&self, path: &str) -> Result<()>;

    /// Append `contents` to `path`, creating it (and parents) if missing,
    /// without disturbing existing content (used for `~/.bashrc`).
    fn append(&self, path: &str, contents: &str) -> Result<()>;

    /// Whether this implementation actually performs effects. `false` for
    /// dry-runs, used to skip steps that only make sense for real installs.
    fn is_real(&self) -> bool;
}

/// Prefix a target-system path with the live mountpoint, e.g.
/// `/etc/hostname` → `/mnt/etc/hostname`.
pub fn target_path(path: &str) -> String {
    format!("{}{}", stack::TARGET_MOUNT, path)
}

/// Compute the device path of the `index`-th partition of `disk`.
///
/// NVMe/MMC devices insert a `p` separator (`/dev/nvme0n1` → `/dev/nvme0n1p1`)
/// while SATA/virtio devices do not (`/dev/vda` → `/dev/vda1`).
pub fn partition_path(disk: &str, index: u32) -> String {
    let needs_p = disk
        .rsplit('/')
        .next()
        .is_some_and(|name| name.chars().last().is_some_and(|c| c.is_ascii_digit()));
    if needs_p {
        format!("{disk}p{index}")
    } else {
        format!("{disk}{index}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partition_path_handles_sata_and_nvme() {
        assert_eq!(partition_path("/dev/vda", 1), "/dev/vda1");
        assert_eq!(partition_path("/dev/sda", 2), "/dev/sda2");
        assert_eq!(partition_path("/dev/nvme0n1", 1), "/dev/nvme0n1p1");
        assert_eq!(partition_path("/dev/mmcblk0", 2), "/dev/mmcblk0p2");
    }

    #[test]
    fn target_path_prefixes_mount() {
        assert_eq!(target_path("/etc/hostname"), "/mnt/etc/hostname");
    }

    #[test]
    fn chroot_wraps_command() {
        let cmd = Command::new("systemctl")
            .arg("enable")
            .arg("NetworkManager")
            .in_chroot();
        assert_eq!(cmd.program, "arch-chroot");
        assert_eq!(cmd.args, ["/mnt", "systemctl", "enable", "NetworkManager"]);
    }

    #[test]
    fn command_display_quotes_whitespace() {
        let cmd = Command::new("echo").arg("hello world").arg("plain");
        assert_eq!(cmd.to_string(), "echo \"hello world\" plain");
    }
}
