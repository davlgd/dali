//! Command-line interface definition.

use std::path::PathBuf;

use clap::Parser;
use clap_complete::Shell;

/// Davlgd Arch Linux Installer — an opinionated, single-binary TUI installer.
#[derive(Debug, Parser)]
#[command(name = "dali", version, about, long_about = None)]
// A CLI arg struct is naturally a flat bag of independent flags.
#[allow(clippy::struct_excessive_bools)]
pub struct Cli {
    /// Install non-interactively from a TOML configuration file.
    ///
    /// If a sibling `<name>.credentials.toml` sits next to it, its passwords
    /// are merged in (so the main file can be kept secret-free).
    #[arg(short, long, value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Print the exact plan of actions without changing anything.
    #[arg(long)]
    pub dry_run: bool,

    /// Skip the final "this will erase the disk" confirmation.
    ///
    /// Only meaningful for non-interactive (`--config`) installs — an
    /// interactive run is already gated by the wizard. Requires `--config`.
    #[arg(long, requires = "config")]
    pub yes: bool,

    /// Write the effective configuration (from `--config`, or from the wizard
    /// if none) to a file and exit without installing. Passwords go into a
    /// sibling `<name>.credentials.toml` (mode 0600), not the main file.
    ///
    /// Cannot be combined with `--dry-run` or `--yes`.
    #[arg(long, value_name = "FILE", conflicts_with_all = ["dry_run", "yes"])]
    pub save_config: Option<PathBuf>,

    /// Do not reboot at the end. By default a finished install reboots into the
    /// new system (immediately with `--yes`, after a confirmation otherwise).
    #[arg(long)]
    pub no_reboot: bool,

    /// Print a shell completion script to stdout and exit (e.g. `bash`, `zsh`,
    /// `fish`).
    #[arg(long, value_name = "SHELL")]
    pub completions: Option<Shell>,

    /// Print a man page (roff) to stdout and exit.
    #[arg(long)]
    pub man: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }
}
