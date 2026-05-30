//! Step 3 — install the base system into the mounted target with `pacstrap`.

use super::{Context, Step};
use crate::config::stack;
use crate::error::Result;
use crate::system::{Command, probe};

/// Runs `pacstrap` with the resolved package set.
pub struct Pacstrap;

impl Step for Pacstrap {
    fn name(&self) -> &'static str {
        "Install base system"
    }

    fn run(&self, ctx: &mut Context<'_>) -> Result<()> {
        let mut packages = ctx.config.all_packages();

        // CPU microcode matches the host being installed onto, so it is probed
        // here rather than carried in the config. The bootloader step wires the
        // matching initrd line.
        if let Some(ucode) = probe::cpu_microcode() {
            ctx.info(format!("adding {ucode} CPU microcode"));
            packages.push(ucode.to_owned());
        }

        ctx.info(format!(
            "installing {} packages (this can take a while)",
            packages.len()
        ));

        // `-K` initialises a fresh pacman keyring inside the new root.
        let command = Command::new("pacstrap")
            .arg("-K")
            .arg(stack::TARGET_MOUNT)
            .args(packages);
        ctx.sys.run(&command)
    }
}
