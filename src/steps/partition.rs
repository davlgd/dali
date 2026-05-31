//! Step 1 — wipe the target disk and lay down a GPT with an EFI System
//! Partition and a single root partition.

use super::{Context, Step};
use crate::config::stack;
use crate::error::Result;
use crate::system::Command;

/// Creates the GPT partition table: `p1` = ESP, `p2` = root (rest of the disk).
pub struct Partition;

impl Step for Partition {
    fn name(&self) -> &'static str {
        "Partition disk"
    }

    fn run(&self, ctx: &mut Context<'_>) -> Result<()> {
        let disk = ctx.config.disk.clone();
        ctx.info(format!("wiping existing signatures on {disk}"));

        // Zap any existing GPT/MBR structures so we start from a clean slate.
        ctx.sys
            .run(&Command::new("sgdisk").arg("--zap-all").arg(&disk))?;
        ctx.sys
            .run(&Command::new("wipefs").arg("--all").arg(&disk))?;

        ctx.info(format!(
            "creating {} MiB EFI partition + Btrfs root",
            stack::ESP_SIZE_MIB
        ));
        ctx.sys.run(
            &Command::new("sgdisk")
                .arg(format!("--new=1:0:+{}M", stack::ESP_SIZE_MIB))
                .arg("--typecode=1:ef00")
                .arg("--change-name=1:EFI")
                .arg("--new=2:0:0")
                .arg("--typecode=2:8304")
                .arg("--change-name=2:root")
                .arg(&disk),
        )?;

        // Make the kernel re-read the new partition table, then wait for udev
        // to create the partition device nodes before anyone formats them.
        ctx.sys.run(&Command::new("partprobe").arg(&disk))?;
        ctx.sys.run(&Command::new("udevadm").arg("settle"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::steps::test_support::{config, dry_actions};

    #[test]
    fn lays_down_esp_and_root_with_correct_typecodes() {
        let joined = dry_actions(&Partition, &config()).join("\n");
        assert!(joined.contains("sgdisk --zap-all /dev/vda"));
        assert!(joined.contains("wipefs --all /dev/vda"));
        assert!(joined.contains("--new=1:0:+1024M"));
        assert!(joined.contains("--typecode=1:ef00"), "ESP type code"); // EFI System
        assert!(joined.contains("--new=2:0:0"));
        assert!(joined.contains("--typecode=2:8304"), "Linux root type code");
        // Re-read the table and wait for the device nodes before formatting.
        assert!(joined.contains("partprobe /dev/vda"));
        assert!(joined.contains("udevadm settle"));
    }
}
