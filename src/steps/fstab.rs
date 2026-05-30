//! Step 4 — generate `/etc/fstab` from the currently mounted layout.

use super::{Context, Step};
use crate::config::stack;
use crate::error::{Error, Result};
use crate::system::{Command, target_path};

/// Captures `genfstab -U /mnt` and writes it into the target.
pub struct GenerateFstab;

impl Step for GenerateFstab {
    fn name(&self) -> &'static str {
        "Generate fstab"
    }

    fn run(&self, ctx: &mut Context<'_>) -> Result<()> {
        // `-U` records filesystems by UUID, which is stable across reboots.
        let fstab = ctx
            .sys
            .capture(&Command::new("genfstab").arg("-U").arg(stack::TARGET_MOUNT))?;

        // On a real run, a silently incomplete fstab (e.g. a missing ESP line)
        // would only surface at first boot. Verify every mountpoint we set up
        // is present before persisting it. (Dry-runs capture "", so skip.)
        if ctx.sys.is_real()
            && let Some(missing) = first_missing_mountpoint(&fstab)
        {
            return Err(Error::Config(format!(
                "generated fstab is missing the `{missing}` entry"
            )));
        }

        ctx.info("writing /etc/fstab (filesystems referenced by UUID)");
        ctx.sys.write(&target_path("/etc/fstab"), &fstab)
    }
}

/// The first expected mountpoint (the Btrfs subvolumes plus the ESP) absent
/// from `fstab`, if any. Mountpoints are matched as whitespace-delimited tokens.
fn first_missing_mountpoint(fstab: &str) -> Option<&'static str> {
    let present = |mountpoint: &str| fstab.split_whitespace().any(|token| token == mountpoint);
    stack::SUBVOLUMES
        .iter()
        .map(|(_, mountpoint)| *mountpoint)
        .chain(std::iter::once(stack::ESP_MOUNT))
        .find(|mountpoint| !present(mountpoint))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn complete_fstab_passes() {
        let fstab = "UUID=x / btrfs subvol=@ 0 0\n\
                     UUID=x /home btrfs subvol=@home 0 0\n\
                     UUID=x /var/log btrfs subvol=@log 0 0\n\
                     UUID=x /var/cache/pacman/pkg btrfs subvol=@pkg 0 0\n\
                     UUID=x /.snapshots btrfs subvol=@snapshots 0 0\n\
                     UUID=y /boot vfat defaults 0 2\n";
        assert_eq!(first_missing_mountpoint(fstab), None);
    }

    #[test]
    fn missing_esp_is_detected() {
        let fstab = "UUID=x / btrfs subvol=@ 0 0\n\
                     UUID=x /home btrfs subvol=@home 0 0\n\
                     UUID=x /var/log btrfs subvol=@log 0 0\n\
                     UUID=x /var/cache/pacman/pkg btrfs subvol=@pkg 0 0\n\
                     UUID=x /.snapshots btrfs subvol=@snapshots 0 0\n";
        assert_eq!(first_missing_mountpoint(fstab), Some("/boot"));
    }

    #[test]
    fn empty_fstab_reports_root_first() {
        assert_eq!(first_missing_mountpoint(""), Some("/"));
    }
}
