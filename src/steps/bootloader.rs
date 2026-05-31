//! Step — install systemd-boot and write a boot entry for the `linux` kernel.

use super::{Context, Step};
use crate::config::stack;
use crate::error::{Error, Result};
use crate::system::{Command, partition_path, probe, target_path};

/// Installs systemd-boot into the ESP and writes loader configuration.
pub struct Bootloader;

impl Step for Bootloader {
    fn name(&self) -> &'static str {
        "Install bootloader"
    }

    fn run(&self, ctx: &mut Context<'_>) -> Result<()> {
        ctx.info("installing systemd-boot into the ESP");
        ctx.sys
            .run(&Command::new("bootctl").arg("install").in_chroot())?;

        // Resolve the root filesystem UUID so the kernel can find its rootfs.
        // `blkid -p` does a low-level probe that bypasses the blkid cache and
        // the udev database — both of which can be stale right after `mkfs`,
        // yielding an empty UUID and an unbootable entry. Cache-based lookups
        // (`blkid <dev>`, `findmnt`, `lsblk`) are unreliable here.
        let root = partition_path(&ctx.config.disk, 2);
        let uuid = ctx
            .sys
            .capture(
                &Command::new("blkid")
                    .arg("--probe")
                    .arg("-s")
                    .arg("UUID")
                    .arg("-o")
                    .arg("value")
                    .arg(&root),
            )?
            .trim()
            .to_owned();
        // A real run with an empty UUID would silently produce an unbootable
        // system; fail loudly instead. (Dry-runs legitimately capture "".)
        if uuid.is_empty() && ctx.sys.is_real() {
            return Err(Error::Config(
                "could not determine the root filesystem UUID".into(),
            ));
        }

        ctx.info("writing loader configuration");
        // The microcode initrd, when present, must come BEFORE the kernel initrd.
        let microcode = probe::cpu_microcode();
        ctx.sys
            .write(&target_path("/boot/loader/loader.conf"), LOADER_CONF)?;
        ctx.sys.write(
            &target_path("/boot/loader/entries/arch.conf"),
            &entry(&uuid, microcode, ctx.config.zram_swap),
        )?;
        Ok(())
    }
}

/// Global loader settings: boot the default entry after a short timeout.
const LOADER_CONF: &str = "default arch.conf\ntimeout 3\nconsole-mode max\neditor no\n";

/// A boot entry pinning the root subvolume and the `linux` kernel images.
///
/// `microcode` is the matching microcode package name (e.g. `intel-ucode`); its
/// initrd line is emitted before the kernel initramfs, as systemd-boot requires.
/// When `zram_swap` is on, zswap is disabled on the cmdline (the two are
/// redundant — zram is the chosen compressed-RAM swap).
fn entry(root_uuid: &str, microcode: Option<&str>, zram_swap: bool) -> String {
    let kernel = stack::KERNEL;
    let (root_subvol, _) = stack::SUBVOLUMES[0];
    let ucode_initrd = microcode.map_or_else(String::new, |pkg| format!("initrd  /{pkg}.img\n"));
    let zswap = if zram_swap { " zswap.enabled=0" } else { "" };
    format!(
        "title   Arch Linux\n\
         linux   /vmlinuz-{kernel}\n\
         {ucode_initrd}\
         initrd  /initramfs-{kernel}.img\n\
         options root=UUID={root_uuid} rootfstype=btrfs rootflags=subvol={root_subvol} rw{zswap}\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_pins_uuid_and_subvolume() {
        let text = entry("dead-beef", None, true);
        assert!(text.contains("root=UUID=dead-beef"));
        assert!(text.contains("rootflags=subvol=@"));
        assert!(text.contains("/vmlinuz-linux"));
        assert!(text.contains("/initramfs-linux.img"));
    }

    #[test]
    fn entry_pins_btrfs_rootfstype() {
        assert!(entry("uuid", None, true).contains("rootfstype=btrfs"));
    }

    #[test]
    fn microcode_initrd_precedes_kernel_initrd() {
        let text = entry("uuid", Some("intel-ucode"), true);
        let ucode = text.find("/intel-ucode.img").unwrap();
        let kernel = text.find("/initramfs-linux.img").unwrap();
        assert!(
            ucode < kernel,
            "microcode initrd must come before the kernel initrd"
        );
    }

    #[test]
    fn zswap_disabled_when_zram_swap_enabled() {
        assert!(entry("uuid", None, true).contains("zswap.enabled=0"));
    }

    #[test]
    fn zswap_param_absent_without_zram_swap() {
        assert!(!entry("uuid", None, false).contains("zswap"));
    }
}
