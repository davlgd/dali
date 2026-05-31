//! Step — write the console login banner and the message of the day.
//!
//! `/etc/issue` uses agetty's `\4` escape, which the console login prompt
//! expands to the machine's live IPv4 address — handy for finding out where to
//! SSH in. `/etc/motd` is a short, factual welcome shown after login.

use super::{Context, Step};
use crate::config::stack;
use crate::error::Result;
use crate::system::target_path;

/// Writes `/etc/issue` and `/etc/motd` into the target.
pub struct LoginBanner;

impl Step for LoginBanner {
    fn name(&self) -> &'static str {
        "Write login banners"
    }

    fn run(&self, ctx: &mut Context<'_>) -> Result<()> {
        ctx.info("writing /etc/issue and /etc/motd");
        ctx.sys.write(&target_path("/etc/issue"), ISSUE)?;
        ctx.sys
            .write(&target_path("/etc/motd"), &motd(&ctx.config.hostname))
    }
}

/// Console pre-login banner. `\4` is an agetty escape expanded to the live IPv4
/// address at display time — it is NOT resolved here.
const ISSUE: &str = "\nArch Linux (provisioned by DALI)\nIPv4: \\4\n\n";

/// Post-login message of the day.
fn motd(hostname: &str) -> String {
    format!(
        "{hostname} - provisioned by DALI {}.\nRun `up` to update the system.\n",
        stack::DALI_VERSION
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::steps::test_support::{config, dry_actions};

    #[test]
    fn writes_issue_and_motd() {
        let actions = dry_actions(&LoginBanner, &config());
        assert!(actions.iter().any(|a| a.contains("/mnt/etc/issue")));
        assert!(actions.iter().any(|a| a.contains("/mnt/etc/motd")));
    }

    #[test]
    fn issue_uses_the_agetty_ip_escape_not_a_resolved_address() {
        assert!(
            ISSUE.contains("\\4"),
            "issue must carry the literal \\4 escape"
        );
    }

    #[test]
    fn motd_is_factual_and_mentions_up() {
        let motd = motd("server01");
        assert!(motd.contains("server01"));
        assert!(motd.contains("provisioned by DALI"));
        assert!(motd.contains("Run `up`"));
        assert!(motd.contains(env!("CARGO_PKG_VERSION")));
    }
}
