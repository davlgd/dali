//! Application wiring: turn parsed [`Cli`] arguments into an installation run.

use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use crate::cli::Cli;
use crate::config::InstallConfig;
use crate::error::{Error, Result};
use crate::report::ConsoleReporter;
use crate::system::{DrySys, RealSys, Sys, probe};
use crate::{steps, tui};

/// Entry point invoked by `main`. Returns `Ok(())` on a completed install,
/// a completed dry-run, a saved config, or a clean user abort.
pub fn run(cli: &Cli) -> Result<()> {
    preflight(cli.dry_run)?;

    // Acquire a configuration: from file (headless) or via the TUI wizard.
    let config = match &cli.config {
        Some(path) => InstallConfig::from_json_file(path)?,
        None => match tui::run_wizard(InstallConfig::default())? {
            Some(config) => config,
            None => return Err(Error::Aborted),
        },
    };
    // Fail fast on a bad file-based config before we even show the summary;
    // `steps::install` re-validates as the authoritative gate on the pipeline.
    config.validate()?;

    if let Some(path) = &cli.save_config {
        return save_config(&config, path);
    }

    // For a real install, catch a non-existent disk or a typo'd timezone now —
    // before partitioning wipes anything — rather than failing mid-pipeline.
    if !cli.dry_run {
        pre_wipe_checks(&config)?;
    }

    // Probe the network once: warn (single place) and surface it in the summary.
    let online = cli.dry_run || probe::has_network();
    if !online {
        eprintln!("warning: no network detected — package installation will likely fail");
    }

    if !cli.dry_run && !cli.yes && !confirm(&config, online, cli.config.as_deref())? {
        return Err(Error::Aborted);
    }

    let mut reporter = ConsoleReporter::new();
    let sys: Box<dyn Sys> = if cli.dry_run {
        println!("\nDry run — the following actions WOULD be performed.");
        println!("(captured command output is unavailable, so derived files like");
        println!(" /etc/fstab show as empty — that is expected in a dry-run.)\n");
        Box::new(DrySys::new())
    } else {
        Box::new(RealSys)
    };

    steps::install(&config, sys.as_ref(), &mut reporter)?;

    if cli.dry_run {
        println!("\nDry run complete. Re-run without --dry-run to install.");
    } else {
        println!("\nInstallation complete. You can reboot into your new system.");
    }
    Ok(())
}

/// Verify the live environment can support an install.
///
/// Real installs require UEFI and root; a dry-run only warns so it can be
/// rehearsed from anywhere.
fn preflight(dry_run: bool) -> Result<()> {
    let uefi = probe::is_uefi();
    let root = probe::is_root();

    if dry_run {
        if !uefi {
            eprintln!("note: not booted in UEFI mode (fine for a dry-run)");
        }
        if !root {
            eprintln!("note: not running as root (fine for a dry-run)");
        }
        return Ok(());
    }

    if !uefi {
        return Err(Error::Environment(
            "DALI requires UEFI boot; this machine booted in legacy BIOS mode".into(),
        ));
    }
    if !root {
        return Err(Error::Environment("DALI must run as root".into()));
    }
    Ok(())
}

/// Checks that must hold before the destructive pipeline runs on a real
/// install: the target disk exists and the timezone is real. Both would
/// otherwise only fail *after* the disk has been wiped.
fn pre_wipe_checks(config: &InstallConfig) -> Result<()> {
    if !Path::new(&config.disk).exists() {
        return Err(Error::Config(format!(
            "target disk `{}` does not exist",
            config.disk
        )));
    }
    let zoneinfo = format!("/usr/share/zoneinfo/{}", config.timezone);
    if !Path::new(&zoneinfo).exists() {
        return Err(Error::Config(format!(
            "unknown timezone `{}` (no {zoneinfo})",
            config.timezone
        )));
    }
    Ok(())
}

/// Persist a configuration to disk (used by `--save-config`).
///
/// The file contains plaintext passwords, so it is created with `0600`
/// permissions rather than relying on the umask.
fn save_config(config: &InstallConfig, path: &Path) -> Result<()> {
    let json = config.to_json()?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|e| Error::io(path, e))?;
    file.write_all(json.as_bytes())
        .map_err(|e| Error::io(path, e))?;
    println!("Configuration written to {} (mode 0600)", path.display());
    println!("warning: this file contains plaintext passwords — keep it safe.");
    Ok(())
}

/// Final, explicit confirmation before destroying data. Shows every decision
/// that materially changes the resulting system — but never the passwords
/// themselves — so the user gives informed consent before the disk is erased.
fn confirm(config: &InstallConfig, online: bool, source: Option<&Path>) -> Result<bool> {
    println!("\nAbout to ERASE {} and install Arch Linux:", config.disk);
    if let Some(path) = source {
        println!("  config   : {}", path.display());
    }
    println!("  hostname : {}", config.hostname);
    println!("  user     : {}", config.user.username);
    println!("  locale   : {} / keymap {}", config.locale, config.keymap);
    println!("  timezone : {}", config.timezone);
    let root_state = if config.root_password.is_empty() {
        "locked (administration via sudo)"
    } else {
        "password set"
    };
    println!("  root     : {root_state}");
    println!(
        "  zram swap: {}",
        if config.zram_swap { "on" } else { "off" }
    );
    if !config.extra_packages.is_empty() {
        println!("  extras   : {}", config.extra_packages.join(", "));
    }
    if !online {
        println!("  network  : NOT DETECTED — package installation will fail partway");
    }
    print!("\nType 'yes' to continue: ");
    io::stdout().flush().map_err(|e| Error::io("<stdout>", e))?;

    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .map_err(|e| Error::io("<stdin>", e))?;
    Ok(answer.trim().eq_ignore_ascii_case("yes"))
}
