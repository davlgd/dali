//! Step — apply small, always-on system tuning aimed at a CLI/dev/server box:
//! sysctl and systemd drop-ins written into the target's `/etc`.

use super::{Context, Step};
use crate::error::Result;
use crate::system::target_path;

/// Writes the DALI system-tuning drop-ins.
pub struct Tuning;

impl Step for Tuning {
    fn name(&self) -> &'static str {
        "Apply system tuning"
    }

    fn run(&self, ctx: &mut Context<'_>) -> Result<()> {
        // A dev box (editors, watchers, node) blows past the default 8192
        // inotify watches almost immediately.
        ctx.info("raising fs.inotify.max_user_watches");
        ctx.sys.write(
            &target_path("/etc/sysctl.d/90-dali-inotify.conf"),
            INOTIFY_CONF,
        )?;

        // Raise the open-file-descriptor ceiling for system and user services
        // (databases, servers, node hit the default quickly).
        ctx.info("raising the systemd DefaultLimitNOFILE");
        ctx.sys.write(
            &target_path("/etc/systemd/system.conf.d/90-dali-nofile.conf"),
            NOFILE_LIMITS,
        )?;
        ctx.sys.write(
            &target_path("/etc/systemd/user.conf.d/90-dali-nofile.conf"),
            NOFILE_LIMITS,
        )
    }
}

/// inotify watch limit, well above the kernel default of 8192.
const INOTIFY_CONF: &str = "fs.inotify.max_user_watches = 524288\n";
/// systemd file-descriptor limit (soft:hard) for system and user managers.
const NOFILE_LIMITS: &str = "[Manager]\nDefaultLimitNOFILE=65536:524288\n";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::steps::test_support::{config, dry_actions};

    #[test]
    fn inotify_sysctl_dropin_is_written() {
        let actions = dry_actions(&Tuning, &config());
        assert!(
            actions
                .iter()
                .any(|a| a.contains("/etc/sysctl.d/90-dali-inotify.conf"))
        );
    }

    #[test]
    fn inotify_limit_value() {
        assert!(INOTIFY_CONF.contains("fs.inotify.max_user_watches = 524288"));
    }

    #[test]
    fn systemd_nofile_dropins_are_written() {
        let actions = dry_actions(&Tuning, &config());
        assert!(
            actions
                .iter()
                .any(|a| a.contains("/etc/systemd/system.conf.d/90-dali-nofile.conf"))
        );
        assert!(
            actions
                .iter()
                .any(|a| a.contains("/etc/systemd/user.conf.d/90-dali-nofile.conf"))
        );
    }

    #[test]
    fn nofile_limit_value() {
        assert!(NOFILE_LIMITS.contains("DefaultLimitNOFILE=65536:524288"));
    }
}
