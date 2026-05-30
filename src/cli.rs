//! Command-line interface definition.

use std::path::PathBuf;

use clap::Parser;

/// Davlgd Arch Linux Installer — an opinionated, single-binary TUI installer.
#[derive(Debug, Parser)]
#[command(name = "dali", version, about, long_about = None)]
pub struct Cli {
    /// Install non-interactively from a JSON configuration file.
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
    /// if none) to a file and exit without installing.
    ///
    /// Cannot be combined with `--dry-run` or `--yes`.
    #[arg(long, value_name = "FILE", conflicts_with_all = ["dry_run", "yes"])]
    pub save_config: Option<PathBuf>,

    /// Do not reboot at the end. By default a finished install reboots into the
    /// new system (immediately with `--yes`, after a confirmation otherwise).
    #[arg(long)]
    pub no_reboot: bool,
}
