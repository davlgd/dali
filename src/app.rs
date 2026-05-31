//! Application wiring: turn parsed [`Cli`] arguments into an installation run.

use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use clap::CommandFactory;
use clap_complete::Shell;

use crate::cli::Cli;
use crate::config::InstallConfig;
use crate::error::{Error, Result};
use crate::report::ConsoleReporter;
use crate::system::{DrySys, RealSys, Sys, probe};
use crate::{steps, tui};

/// Entry point invoked by `main`. Returns `Ok(())` on a completed install,
/// a completed dry-run, a saved config, or a clean user abort.
pub fn run(cli: &Cli) -> Result<()> {
    // Generator flags short-circuit before any environment checks.
    if let Some(shell) = cli.completions {
        print_completions(shell);
        return Ok(());
    }
    if cli.man {
        return print_man();
    }

    preflight(cli.dry_run)?;

    // Acquire a configuration: from file (headless) or via the TUI wizard.
    let config = match &cli.config {
        Some(path) => InstallConfig::from_toml_file(path)?,
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
        let offer = if cli.no_reboot {
            RebootOffer::Skip
        } else if cli.yes {
            RebootOffer::Auto
        } else {
            RebootOffer::Ask
        };
        finish_install(offer, sys.as_ref());
    }
    Ok(())
}

/// Print a shell completion script for `shell` to stdout.
fn print_completions(shell: Shell) {
    let mut cmd = Cli::command();
    clap_complete::generate(shell, &mut cmd, "dali", &mut io::stdout());
}

/// Print the man page (roff) to stdout.
fn print_man() -> Result<()> {
    clap_mangen::Man::new(Cli::command())
        .render(&mut io::stdout())
        .map_err(|e| Error::io("<stdout>", e))
}

/// What to do at the end of a real install. Rebooting is the default action;
/// `--no-reboot` opts out.
#[derive(Clone, Copy)]
enum RebootOffer {
    /// Interactive run: ask, defaulting to reboot.
    Ask,
    /// Non-interactive (`--yes`): reboot immediately.
    Auto,
    /// `--no-reboot`: leave the machine running.
    Skip,
}

/// Report success and, by default, reboot straight into the new system.
fn finish_install(offer: RebootOffer, sys: &dyn Sys) {
    println!("\nInstallation complete.");
    let reboot = match offer {
        RebootOffer::Skip => {
            println!("Reboot into your new system when ready (e.g. `reboot`).");
            false
        }
        RebootOffer::Auto => true,
        RebootOffer::Ask => {
            prompt_yes_no("Reboot now into your new system?", true).unwrap_or(false)
        }
    };
    if reboot {
        println!("Rebooting…");
        // On success the process goes away with the machine; if it returns,
        // surface a hint.
        let _ = sys.run(&crate::system::Command::new("systemctl").arg("reboot"));
        println!("Could not trigger reboot automatically; run `reboot` yourself.");
    }
}

/// Prompt a yes/no question on the console. `default_yes` sets the answer used
/// for an empty (just-Enter) response and the `[Y/n]` vs `[y/N]` hint.
fn prompt_yes_no(question: &str, default_yes: bool) -> Result<bool> {
    let hint = if default_yes { "[Y/n]" } else { "[y/N]" };
    print!("{question} {hint}: ");
    io::stdout().flush().map_err(|e| Error::io("<stdout>", e))?;
    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .map_err(|e| Error::io("<stdin>", e))?;
    Ok(parse_yes_no(&answer, default_yes))
}

/// Interpret a yes/no answer: empty (just-Enter) takes `default_yes`, otherwise
/// only `y`/`yes` (any case) is a yes.
fn parse_yes_no(answer: &str, default_yes: bool) -> bool {
    let answer = answer.trim();
    if answer.is_empty() {
        default_yes
    } else {
        answer.eq_ignore_ascii_case("y") || answer.eq_ignore_ascii_case("yes")
    }
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
/// install. Everything here fails *before* any disk is touched, so a typo or a
/// wrong target can never wipe the wrong device or leave a half-installed box.
fn pre_wipe_checks(config: &InstallConfig) -> Result<()> {
    // The target must be a whole disk DALI actually enumerates. This rejects
    // partitions, regular files, and the live media in one check — none of
    // those appear in `list_disks`.
    let disks = probe::list_disks();
    if !disks.iter().any(|d| d.path == config.disk) {
        let available: Vec<&str> = disks.iter().map(|d| d.path.as_str()).collect();
        let listed = if available.is_empty() {
            "none detected".to_owned()
        } else {
            available.join(", ")
        };
        return Err(Error::Config(format!(
            "`{}` is not an installable whole disk (available: {listed})",
            config.disk
        )));
    }

    // Never erase a disk that currently backs a mounted filesystem.
    if probe::disk_is_mounted(&config.disk) {
        return Err(Error::Config(format!(
            "`{}` or one of its partitions is mounted — refusing to erase it",
            config.disk
        )));
    }

    // Catch typo'd locale / keymap / timezone now rather than mid-pipeline.
    require_exists(
        &format!("/usr/share/zoneinfo/{}", config.timezone),
        &format!("unknown timezone `{}`", config.timezone),
    )?;
    let locale_base = config.locale.split('.').next().unwrap_or(&config.locale);
    require_exists(
        &format!("/usr/share/i18n/locales/{locale_base}"),
        &format!("unknown locale `{}`", config.locale),
    )?;
    if !probe::keymap_exists(&config.keymap) {
        return Err(Error::Config(format!(
            "unknown console keymap `{}`",
            config.keymap
        )));
    }
    Ok(())
}

/// Whether `path` exists, mapping absence to a descriptive config error.
fn require_exists(path: &str, message: &str) -> Result<()> {
    if Path::new(path).exists() {
        Ok(())
    } else {
        Err(Error::Config(format!("{message} (no {path})")))
    }
}

/// Persist a configuration to disk (used by `--save-config`).
///
/// Splits into two files: the shareable config (no passwords) at `path`, and a
/// sidecar `*.credentials.toml` holding the plaintext passwords at `0600`. The
/// credentials file is written *after* the safe one, so a partial failure never
/// leaves secrets in the world-readable file.
fn save_config(config: &InstallConfig, path: &Path) -> Result<()> {
    std::fs::write(path, config.to_toml_safe()?).map_err(|e| Error::io(path, e))?;

    let creds_path = crate::config::credentials_path(path);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&creds_path)
        .map_err(|e| Error::io(&creds_path, e))?;
    file.write_all(config.to_credentials_toml()?.as_bytes())
        .map_err(|e| Error::io(&creds_path, e))?;

    println!("Configuration written to {} (no secrets)", path.display());
    println!(
        "Credentials written to {} (mode 0600)",
        creds_path.display()
    );
    println!("warning: the credentials file contains plaintext passwords — keep it safe.");
    Ok(())
}

/// Final, explicit confirmation before destroying data. Shows the key
/// destructive choices — target disk, identity, localization, root state, zram
/// and extra packages — but never the passwords themselves, so the user gives
/// informed consent before the disk is erased. (SSH key import is listed in the
/// interactive TUI summary rather than here.)
fn confirm(config: &InstallConfig, online: bool, source: Option<&Path>) -> Result<bool> {
    println!();
    for line in confirm_summary(config, online, source) {
        println!("{line}");
    }
    print!("\nType 'yes' to continue: ");
    io::stdout().flush().map_err(|e| Error::io("<stdout>", e))?;

    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .map_err(|e| Error::io("<stdin>", e))?;
    Ok(answer.trim().eq_ignore_ascii_case("yes"))
}

/// The lines of the pre-install summary (everything but the typed prompt). The
/// `config`/`extras` lines appear only when relevant, and the network warning
/// only when offline.
fn confirm_summary(config: &InstallConfig, online: bool, source: Option<&Path>) -> Vec<String> {
    let mut lines = vec![format!(
        "About to ERASE {} and install Arch Linux:",
        config.disk
    )];
    if let Some(path) = source {
        lines.push(format!("  config   : {}", path.display()));
    }
    lines.push(format!("  hostname : {}", config.hostname));
    lines.push(format!("  user     : {}", config.user.username));
    lines.push(format!(
        "  locale   : {} / keymap {}",
        config.locale, config.keymap
    ));
    lines.push(format!("  timezone : {}", config.timezone));
    let root_state = if config.root_password.is_empty() {
        "locked (administration via sudo)"
    } else {
        "password set"
    };
    lines.push(format!("  root     : {root_state}"));
    lines.push(format!(
        "  zram swap: {}",
        if config.zram_swap { "on" } else { "off" }
    ));
    if !config.extra_packages.is_empty() {
        lines.push(format!("  extras   : {}", config.extra_packages.join(", ")));
    }
    if !online {
        lines.push("  network  : NOT DETECTED — package installation will fail partway".to_owned());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Secret;

    #[test]
    fn parse_yes_no_uses_default_on_empty_answer() {
        assert!(parse_yes_no("", true));
        assert!(!parse_yes_no("", false));
        assert!(!parse_yes_no("   \n", false));
    }

    #[test]
    fn parse_yes_no_accepts_only_y_or_yes() {
        for yes in ["y", "Y", "yes", "YES", " Yes "] {
            assert!(parse_yes_no(yes, false), "{yes:?} should read as yes");
        }
        for no in ["n", "no", "nope", "sure", "1"] {
            assert!(!parse_yes_no(no, true), "{no:?} should read as no");
        }
    }

    #[test]
    fn confirm_summary_shows_core_choices_and_hides_optional_ones() {
        let config = InstallConfig {
            disk: "/dev/vda".to_owned(),
            ..InstallConfig::default()
        };
        let joined = confirm_summary(&config, true, None).join("\n");
        assert!(joined.contains("About to ERASE /dev/vda"));
        assert!(joined.contains("root     : locked"));
        assert!(joined.contains("zram swap: on"));
        // Optional lines are absent on the default, online path.
        assert!(!joined.contains("config   :"));
        assert!(!joined.contains("extras   :"));
        assert!(!joined.contains("network  :"));
    }

    #[test]
    fn confirm_summary_includes_optional_lines_when_relevant() {
        let config = InstallConfig {
            disk: "/dev/sda".to_owned(),
            root_password: Secret::new("rootpw"),
            extra_packages: vec!["htop".to_owned(), "git".to_owned()],
            ..InstallConfig::default()
        };
        let path = std::path::PathBuf::from("/tmp/cfg.toml");
        let joined = confirm_summary(&config, false, Some(&path)).join("\n");
        assert!(joined.contains("config   : /tmp/cfg.toml"));
        assert!(joined.contains("root     : password set"));
        assert!(joined.contains("extras   : htop, git"));
        assert!(joined.contains("network  : NOT DETECTED"));
    }
}
