//! Step 2 — create filesystems, the Btrfs subvolume layout, and mount
//! everything under the live mountpoint ready for `pacstrap`.

use super::{Context, Step};
use crate::config::stack;
use crate::error::Result;
use crate::system::{Command, partition_path, target_path};

/// Mount options applied to every Btrfs subvolume.
const BTRFS_OPTS: &str = "compress=zstd,noatime";
/// ESP mount options: restrict the world-readable FAT to root so genfstab
/// records hardened permissions on `/boot` (kernel images, loader entries).
const ESP_OPTS: &str = "fmask=0077,dmask=0077";

/// Formats the ESP (FAT32) and root (Btrfs), creates subvolumes, and mounts.
pub struct FormatAndMount;

impl Step for FormatAndMount {
    fn name(&self) -> &'static str {
        "Create filesystems and mount"
    }

    fn run(&self, ctx: &mut Context<'_>) -> Result<()> {
        let disk = &ctx.config.disk;
        let esp = partition_path(disk, 1);
        let root = partition_path(disk, 2);
        let mnt = stack::TARGET_MOUNT;

        ctx.info(format!("formatting {esp} as FAT32 and {root} as Btrfs"));
        ctx.sys.run(
            &Command::new("mkfs.fat")
                .arg("-F32")
                .arg("-n")
                .arg("EFI")
                .arg(&esp),
        )?;
        ctx.sys.run(
            &Command::new("mkfs.btrfs")
                .arg("-f")
                .arg("-L")
                .arg("root")
                .arg(&root),
        )?;

        // Mount the top-level volume to create subvolumes, then remount the
        // root subvolume with our options.
        ctx.sys.run(&Command::new("mount").arg(&root).arg(mnt))?;
        for (subvol, _) in stack::SUBVOLUMES {
            ctx.sys.run(
                &Command::new("btrfs")
                    .arg("subvolume")
                    .arg("create")
                    .arg(format!("{mnt}/{subvol}")),
            )?;
        }
        ctx.sys.run(&Command::new("umount").arg(mnt))?;

        ctx.info("mounting subvolume layout");
        // Mount the root subvolume first; the rest hang off it.
        let (root_subvol, _) = stack::SUBVOLUMES[0];
        ctx.sys.run(&mount_subvol(&root, root_subvol, mnt))?;

        for (subvol, rel) in &stack::SUBVOLUMES[1..] {
            let mountpoint = target_path(rel);
            ctx.sys.mkdir_p(&mountpoint)?;
            ctx.sys.run(&mount_subvol(&root, subvol, &mountpoint))?;
        }

        // Mount the EFI System Partition at /boot inside the target.
        let esp_mount = target_path(stack::ESP_MOUNT);
        ctx.sys.mkdir_p(&esp_mount)?;
        ctx.sys.run(
            &Command::new("mount")
                .arg("-o")
                .arg(ESP_OPTS)
                .arg(&esp)
                .arg(&esp_mount),
        )?;
        Ok(())
    }
}

/// Build a `mount -o subvol=<name>,<opts> <device> <mountpoint>` command.
fn mount_subvol(device: &str, subvol: &str, mountpoint: &str) -> Command {
    Command::new("mount")
        .arg("-o")
        .arg(format!("subvol={subvol},{BTRFS_OPTS}"))
        .arg(device)
        .arg(mountpoint)
}
