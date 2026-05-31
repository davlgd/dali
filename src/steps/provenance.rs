//! Step — record that the system was provisioned by DALI, without pretending to
//! be a different distribution.
//!
//! Writes `/etc/dali-release` (a small provenance marker the future `up`/upgrade
//! tooling can key off) and adds *additive* `DALI_*` fields to `/etc/os-release`
//! while keeping `ID=arch` and `PRETTY_NAME="Arch Linux"`, so distro-detection
//! tooling is unaffected. `/etc/os-release` is normally a symlink into the
//! `filesystem` package's `/usr/lib/os-release`; it is replaced with a real
//! file so the package file is never corrupted.

use super::{Context, Step};
use crate::config::stack;
use crate::error::Result;
use crate::system::{Command, target_path};

/// Writes the provenance marker and extends os-release.
pub struct Provenance;

impl Step for Provenance {
    fn name(&self) -> &'static str {
        "Write provenance marker"
    }

    fn run(&self, ctx: &mut Context<'_>) -> Result<()> {
        ctx.info("writing /etc/dali-release");
        ctx.sys
            .write(&target_path("/etc/dali-release"), &dali_release())?;

        // /etc/os-release is a symlink to ../usr/lib/os-release; replace it with
        // a real file so we extend it without touching the package's copy.
        ctx.sys.run(
            &Command::new("rm")
                .arg("-f")
                .arg("/etc/os-release")
                .in_chroot(),
        )?;
        let base = ctx
            .sys
            .capture(&Command::new("cat").arg("/usr/lib/os-release").in_chroot())
            .unwrap_or_default();
        let base = if base.trim().is_empty() {
            ARCH_OS_RELEASE_FALLBACK.to_owned()
        } else {
            base
        };
        let extra = format!(
            "\nDALI_VERSION={}\nDALI_VARIANT=minimal\n",
            stack::DALI_VERSION
        );
        ctx.sys
            .write(&target_path("/etc/os-release"), &format!("{base}{extra}"))
    }
}

/// The `/etc/dali-release` provenance marker.
fn dali_release() -> String {
    format!(
        "NAME=\"DALI\"\nID=dali\nID_LIKE=arch\nVERSION=\"{}\"\nUPSTREAM=arch\n",
        stack::DALI_VERSION
    )
}

/// Minimal stand-in used when the upstream os-release can't be read (dry-run),
/// so the rewritten file still reports `ID=arch`.
const ARCH_OS_RELEASE_FALLBACK: &str =
    "NAME=\"Arch Linux\"\nPRETTY_NAME=\"Arch Linux\"\nID=arch\nBUILD_ID=rolling\n";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::steps::test_support::{config, dry_actions};

    #[test]
    fn writes_the_dali_release_marker() {
        let actions = dry_actions(&Provenance, &config());
        assert!(actions.iter().any(|a| a.contains("/mnt/etc/dali-release")));
    }

    #[test]
    fn dali_release_identifies_dali_with_version() {
        let marker = dali_release();
        assert!(marker.contains("NAME=\"DALI\""));
        assert!(marker.contains("ID=dali"));
        assert!(marker.contains(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn os_release_keeps_arch_id_and_adds_dali_fields() {
        let extra = format!(
            "\nDALI_VERSION={}\nDALI_VARIANT=minimal\n",
            stack::DALI_VERSION
        );
        let rewritten = format!("{ARCH_OS_RELEASE_FALLBACK}{extra}");
        assert!(rewritten.contains("ID=arch"));
        assert!(rewritten.contains("PRETTY_NAME=\"Arch Linux\""));
        assert!(rewritten.contains("DALI_VERSION="));
    }

    #[test]
    fn removes_os_release_symlink_before_rewriting_it() {
        let actions = dry_actions(&Provenance, &config());
        let rm = actions
            .iter()
            .position(|a| a.contains("rm -f /etc/os-release"))
            .expect("os-release symlink removed");
        let write = actions
            .iter()
            .position(|a| a.contains("/mnt/etc/os-release"))
            .expect("os-release rewritten");
        assert!(rm < write, "must remove the symlink before writing");
    }
}
