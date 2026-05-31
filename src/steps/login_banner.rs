//! Step — show the machine's LAN IPv4 at the console login prompt, so you know
//! where to SSH in.
//!
//! agetty's `\4` escape would pick the first interface — often `docker0` on a
//! Docker host — so instead a NetworkManager dispatcher script regenerates
//! `/etc/issue` with the real egress address (`ip route get`) whenever the
//! network changes. No message of the day: it only duplicated this banner.

use super::{Context, Step};
use crate::error::Result;
use crate::system::{Command, target_path};

/// Installs the login-banner dispatcher and a placeholder `/etc/issue`.
pub struct LoginBanner;

impl Step for LoginBanner {
    fn name(&self) -> &'static str {
        "Configure login banner"
    }

    fn run(&self, ctx: &mut Context<'_>) -> Result<()> {
        ctx.info("installing the login-banner dispatcher (/etc/issue with the LAN IP)");
        // Shown until the network comes up and the dispatcher refreshes it.
        ctx.sys
            .write(&target_path("/etc/issue"), ISSUE_PLACEHOLDER)?;

        ctx.sys.write(&target_path(DISPATCHER), ISSUE_DISPATCHER)?;
        ctx.sys.run(
            &Command::new("chmod")
                .arg("0755")
                .arg(DISPATCHER)
                .in_chroot(),
        )
    }
}

/// NetworkManager dispatcher path (runs root-owned, on connectivity changes).
const DISPATCHER: &str = "/etc/NetworkManager/dispatcher.d/90-dali-issue";

/// Initial banner before the first connection comes up.
const ISSUE_PLACEHOLDER: &str = "\nArch Linux (DALI)\nIPv4: (pending)\n\n";

/// Dispatcher script: rewrite `/etc/issue` with the LAN egress IPv4. Using
/// `ip route get` picks the address used to reach the internet (the LAN one),
/// never the `docker0` bridge.
const ISSUE_DISPATCHER: &str = r#"#!/bin/sh
# DALI: keep /etc/issue showing the machine's LAN IPv4 (handy for SSH).
ip=$(ip -4 route get 1.1.1.1 2>/dev/null | sed -n 's/.*src \([0-9.]*\).*/\1/p')
printf '\nArch Linux (DALI)\nIPv4: %s\n\n' "${ip:-(pending)}" > /etc/issue
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::steps::test_support::{config, dry_actions};

    #[test]
    fn installs_issue_and_the_dispatcher() {
        let actions = dry_actions(&LoginBanner, &config());
        assert!(actions.iter().any(|a| a.contains("/mnt/etc/issue")));
        assert!(
            actions
                .iter()
                .any(|a| a.contains("/mnt/etc/NetworkManager/dispatcher.d/90-dali-issue"))
        );
        assert!(actions.iter().any(|a| a.contains("chmod 0755")
            && a.contains("/etc/NetworkManager/dispatcher.d/90-dali-issue")));
    }

    #[test]
    fn dispatcher_derives_the_lan_ip_not_the_agetty_escape() {
        // Must compute the egress IP (excludes docker0), not rely on `\4`.
        assert!(ISSUE_DISPATCHER.contains("ip -4 route get 1.1.1.1"));
        assert!(!ISSUE_DISPATCHER.contains("\\4"));
    }
}
