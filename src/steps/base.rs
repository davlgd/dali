//! Step — install the base system into the mounted target with `pacstrap`.

use super::{Context, Step};
use crate::config::stack;
use crate::error::Result;
use crate::system::{Command, probe, target_path};

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

        // Reliable keyservers on the live system before pacstrap initialises the
        // keyring, then in the target so the installed system inherits them.
        ctx.sys.mkdir_p("/etc/gnupg")?;
        ctx.sys
            .write("/etc/gnupg/dirmngr.conf", stack::DIRMNGR_CONF)?;

        // `-K` initialises a fresh pacman keyring inside the new root.
        let command = Command::new("pacstrap")
            .arg("-K")
            .arg(stack::TARGET_MOUNT)
            .args(packages);
        ctx.sys.run(&command)?;

        ctx.sys.mkdir_p(&target_path("/etc/gnupg"))?;
        ctx.sys
            .write(&target_path("/etc/gnupg/dirmngr.conf"), stack::DIRMNGR_CONF)?;

        // pacstrap does not copy the host pacman.conf, so apply the same tuning
        // to the target's own (shipped by the `pacman` package).
        let pacman_conf = target_path("/etc/pacman.conf");
        if let Some(body) = probe::read_file(&pacman_conf) {
            ctx.sys
                .write(&pacman_conf, &super::host_pacman::tune_pacman_conf(&body))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::steps::test_support::{config, dry_actions};

    #[test]
    fn dirmngr_conf_written_on_host_before_pacstrap_and_in_target_after() {
        let actions = dry_actions(&Pacstrap, &config());
        // The host write has no `/mnt` prefix; the target one does.
        let host = actions
            .iter()
            .position(|a| a.contains("write: /etc/gnupg/dirmngr.conf"))
            .expect("host dirmngr.conf write");
        let pacstrap = actions
            .iter()
            .position(|a| a.contains("pacstrap"))
            .expect("pacstrap run");
        let target = actions
            .iter()
            .position(|a| a.contains("/mnt/etc/gnupg/dirmngr.conf"))
            .expect("target dirmngr.conf write");
        assert!(host < pacstrap, "host dirmngr.conf must precede pacstrap");
        assert!(
            pacstrap < target,
            "target dirmngr.conf must follow pacstrap"
        );
    }

    #[test]
    fn dirmngr_conf_lists_keyservers_and_timeout() {
        assert!(stack::DIRMNGR_CONF.contains("hkps://"));
        assert!(stack::DIRMNGR_CONF.contains("connect-quick-timeout 4"));
    }
}
