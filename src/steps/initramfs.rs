//! Step — (re)build the initramfs for every installed kernel preset.
//!
//! `pacstrap` already triggers a build when the kernel is installed, but we
//! regenerate explicitly so the result reflects any configuration written in
//! earlier steps and so a failure here surfaces clearly rather than silently.

use super::{Context, Step};
use crate::error::Result;
use crate::system::Command;

/// Runs `mkinitcpio -P` inside the target.
pub struct Initramfs;

impl Step for Initramfs {
    fn name(&self) -> &'static str {
        "Build initramfs"
    }

    fn run(&self, ctx: &mut Context<'_>) -> Result<()> {
        ctx.info("regenerating initramfs for all presets");
        ctx.sys
            .run(&Command::new("mkinitcpio").arg("-P").in_chroot())
    }
}
