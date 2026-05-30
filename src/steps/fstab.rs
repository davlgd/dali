//! Step 4 — generate `/etc/fstab` from the currently mounted layout.

use super::{Context, Step};
use crate::config::stack;
use crate::error::Result;
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
        ctx.info("writing /etc/fstab (filesystems referenced by UUID)");
        ctx.sys.write(&target_path("/etc/fstab"), &fstab)
    }
}
