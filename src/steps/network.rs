//! Step — carry the live ISO's network profiles into the target, so a system
//! installed over Wi-Fi (or a configured wired connection) comes back online
//! after the first reboot without re-entering credentials.
//!
//! Best-effort: if nothing was configured on the live system, it is a clean
//! no-op. Profiles can hold secrets, so they are written `0600` in a `0700`
//! directory (NetworkManager refuses world-readable connection files).

use std::path::Path;

use super::{Context, Step};
use crate::error::Result;
use crate::system::{Command, probe, target_path};

/// Copies NetworkManager / iwd profiles from the live system into the target.
pub struct CarryNetwork;

impl Step for CarryNetwork {
    fn name(&self) -> &'static str {
        "Carry network configuration"
    }

    fn run(&self, ctx: &mut Context<'_>) -> Result<()> {
        carry_dir(ctx, probe::NM_CONNECTIONS_DIR)?;
        carry_dir(ctx, probe::IWD_DIR)
    }
}

/// Copy every profile file from the live `dir` into the same path in the
/// target, then lock down permissions. No-op when the live `dir` is empty.
fn carry_dir(ctx: &mut Context<'_>, dir: &str) -> Result<()> {
    let files = collect_files(ctx, dir);
    if files.is_empty() {
        return Ok(());
    }

    ctx.info(format!(
        "copying {} network profile(s) from {dir}",
        files.len()
    ));
    ctx.sys.mkdir_p(&target_path(dir))?;
    for (name, contents) in &files {
        ctx.sys
            .write(&format!("{}/{name}", target_path(dir)), contents)?;
    }
    // Per-dir 0700, per-file 0600 (not `chmod -R 600`, which would strip the
    // directory's execute bit). Paths are chroot-relative.
    ctx.sys
        .run(&Command::new("chmod").arg("700").arg(dir).in_chroot())?;
    for (name, _) in &files {
        ctx.sys.run(
            &Command::new("chmod")
                .arg("600")
                .arg(format!("{dir}/{name}"))
                .in_chroot(),
        )?;
    }
    Ok(())
}

/// `(filename, contents)` for each profile in the live `dir`. On a dry-run the
/// live filesystem cannot be enumerated, so a single representative entry is
/// returned to make the planned actions visible.
fn collect_files(ctx: &Context<'_>, dir: &str) -> Vec<(String, String)> {
    if ctx.sys.is_real() {
        probe::list_files_in(Path::new(dir))
            .into_iter()
            .filter_map(|path| {
                let name = path.file_name()?.to_string_lossy().into_owned();
                let contents = std::fs::read_to_string(&path).ok()?;
                Some((name, contents))
            })
            .collect()
    } else {
        vec![("<live-profile>".to_owned(), String::new())]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::steps::test_support::{config, dry_actions};

    #[test]
    fn copies_and_locks_down_network_profiles() {
        let actions = dry_actions(&CarryNetwork, &config());
        let nm = probe::NM_CONNECTIONS_DIR;
        assert!(
            actions
                .iter()
                .any(|a| a.contains(&format!("mkdir -p /mnt{nm}")))
        );
        assert!(
            actions
                .iter()
                .any(|a| a.contains(&format!("/mnt{nm}/<live-profile>")))
        );
        assert!(
            actions
                .iter()
                .any(|a| a.contains(&format!("chmod 700 {nm}")))
        );
        assert!(
            actions
                .iter()
                .any(|a| a.contains(&format!("chmod 600 {nm}/<live-profile>")))
        );
    }

    #[test]
    fn also_carries_the_iwd_directory() {
        let actions = dry_actions(&CarryNetwork, &config());
        assert!(actions.iter().any(|a| a.contains(probe::IWD_DIR)));
    }
}
