//! Step — opinionated security hardening for the default app set: an sshd
//! drop-in and a `ufw` firewall.
//!
//! Both `openssh` and `ufw` come from the curated app set, so this step is a
//! no-op when `default_apps` is off. The firewall is configured by a one-shot
//! service on first boot rather than in the chroot: `ufw`'s netfilter backend
//! isn't reliably available under `arch-chroot`, and getting `deny incoming`
//! applied without the matching `allow ssh` would lock the machine out.

use super::{Context, Step};
use crate::error::Result;
use crate::system::{Command, target_path};

/// Writes the sshd drop-in and installs the first-boot firewall setup.
pub struct Harden;

impl Step for Harden {
    fn name(&self) -> &'static str {
        "Harden the system (sshd, firewall)"
    }

    fn run(&self, ctx: &mut Context<'_>) -> Result<()> {
        if !ctx.config.default_apps {
            ctx.info("skipped (default app set disabled)");
            return Ok(());
        }

        ctx.info("writing the sshd hardening drop-in");
        ctx.sys.write(
            &target_path("/etc/ssh/sshd_config.d/10-dali-hardening.conf"),
            SSHD_HARDENING,
        )?;

        ctx.info("installing the first-boot firewall setup (ufw)");
        ctx.sys
            .write(&target_path(FIREWALL_SCRIPT), FIREWALL_SETUP)?;
        ctx.sys.run(
            &Command::new("chmod")
                .arg("0755")
                .arg(FIREWALL_SCRIPT)
                .in_chroot(),
        )?;
        ctx.sys.write(
            &target_path("/etc/systemd/system/dali-firewall.service"),
            FIREWALL_SERVICE,
        )?;
        ctx.sys.run(
            &Command::new("systemctl")
                .arg("enable")
                .arg("dali-firewall.service")
                .in_chroot(),
        )
    }
}

/// sshd hardening: root is already locked by default, so disable root SSH
/// outright and cap auth attempts.
const SSHD_HARDENING: &str = "PermitRootLogin no\nMaxAuthTries 3\n";

/// First-boot firewall script path.
const FIREWALL_SCRIPT: &str = "/usr/local/bin/dali-firewall";

/// Configure ufw on the running system (deny incoming, keep SSH reachable),
/// enable it, then disable this one-shot so it never runs again.
const FIREWALL_SETUP: &str = r"#!/bin/sh
# DALI: configure the firewall on first boot, where ufw's backend works.
# `set -e` so a failed step leaves the one-shot enabled to retry next boot
# rather than self-disabling with the firewall unconfigured.
set -e
ufw default deny incoming
ufw default allow outgoing
ufw allow ssh
ufw --force enable
systemctl disable dali-firewall.service
";

/// One-shot unit that runs the firewall setup once the network stack is up.
const FIREWALL_SERVICE: &str = "[Unit]\n\
    Description=DALI first-boot firewall setup\n\
    After=network.target\n\
    ConditionPathExists=/usr/bin/ufw\n\
    \n\
    [Service]\n\
    Type=oneshot\n\
    ExecStart=/usr/local/bin/dali-firewall\n\
    \n\
    [Install]\n\
    WantedBy=multi-user.target\n";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::steps::test_support::{config, dry_actions};

    #[test]
    fn skipped_without_the_default_app_set() {
        let mut cfg = config();
        cfg.default_apps = false;
        assert!(dry_actions(&Harden, &cfg).is_empty());
    }

    #[test]
    fn writes_sshd_dropin_and_firewall_setup() {
        let actions = dry_actions(&Harden, &config());
        assert!(
            actions
                .iter()
                .any(|a| a.contains("/etc/ssh/sshd_config.d/10-dali-hardening.conf"))
        );
        assert!(
            actions
                .iter()
                .any(|a| a.contains("/usr/local/bin/dali-firewall"))
        );
        assert!(
            actions
                .iter()
                .any(|a| a.contains("systemctl enable dali-firewall.service"))
        );
    }

    #[test]
    fn sshd_disables_root_login_and_firewall_keeps_ssh() {
        assert!(SSHD_HARDENING.contains("PermitRootLogin no"));
        // The firewall must allow SSH, or enabling deny-incoming locks us out.
        assert!(FIREWALL_SETUP.contains("ufw allow ssh"));
        assert!(FIREWALL_SETUP.contains("deny incoming"));
        // `set -e` so a failed run retries instead of self-disabling.
        assert!(FIREWALL_SETUP.contains("set -e"));
    }
}
