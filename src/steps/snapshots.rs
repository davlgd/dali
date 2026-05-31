//! Step — configure snapper for root snapshots on the existing `@snapshots`
//! subvolume, with `snap-pac` taking pre/post snapshots around pacman.
//!
//! The snapper config is written directly rather than via `snapper
//! create-config`, which would try to create its own `/.snapshots` subvolume
//! and collide with the `@snapshots` subvolume already created and mounted by
//! the storage step. Only `/` is snapshotted — `/home` is user data and rolling
//! it back would lose work. DALI never enables btrfs quotas, so there are none
//! to disable.

use super::{Context, Step};
use crate::error::Result;
use crate::system::{Command, target_path};

/// Sets up the snapper `root` config and locks down `/.snapshots`.
pub struct Snapshots;

impl Step for Snapshots {
    fn name(&self) -> &'static str {
        "Configure snapshots (snapper)"
    }

    fn run(&self, ctx: &mut Context<'_>) -> Result<()> {
        ctx.info("writing snapper root configuration");
        ctx.sys
            .write(&target_path("/etc/snapper/configs/root"), ROOT_CONFIG)?;
        ctx.sys.write(
            &target_path("/etc/conf.d/snapper"),
            "SNAPPER_CONFIGS=\"root\"\n",
        )?;

        // snapper requires /.snapshots to be root-owned and not group/world
        // accessible.
        ctx.sys.run(
            &Command::new("chmod")
                .arg("0750")
                .arg("/.snapshots")
                .in_chroot(),
        )?;
        ctx.sys.run(
            &Command::new("chown")
                .arg("root:root")
                .arg("/.snapshots")
                .in_chroot(),
        )
    }
}

/// The snapper `root` config: root subvolume only, no timeline timer (snap-pac
/// covers pacman pre/post), wheel allowed to manage snapshots, no qgroup.
const ROOT_CONFIG: &str = r#"SUBVOLUME="/"
FSTYPE="btrfs"
QGROUP=""
SPACE_LIMIT="0.5"
FREE_LIMIT="0.2"
ALLOW_USERS=""
ALLOW_GROUPS="wheel"
SYNC_ACL="yes"
BACKGROUND_COMPARISON="yes"
NUMBER_CLEANUP="yes"
NUMBER_MIN_AGE="1800"
NUMBER_LIMIT="10"
NUMBER_LIMIT_IMPORTANT="10"
TIMELINE_CREATE="no"
TIMELINE_CLEANUP="yes"
TIMELINE_MIN_AGE="1800"
TIMELINE_LIMIT_HOURLY="10"
TIMELINE_LIMIT_DAILY="7"
TIMELINE_LIMIT_WEEKLY="0"
TIMELINE_LIMIT_MONTHLY="0"
TIMELINE_LIMIT_YEARLY="0"
EMPTY_PRE_POST_CLEANUP="yes"
EMPTY_PRE_POST_MIN_AGE="1800"
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::steps::test_support::{config, dry_actions};

    #[test]
    fn writes_root_config_and_registers_it() {
        let actions = dry_actions(&Snapshots, &config());
        assert!(
            actions
                .iter()
                .any(|a| a.contains("/etc/snapper/configs/root"))
        );
        assert!(actions.iter().any(|a| a.contains("/etc/conf.d/snapper")));
    }

    #[test]
    fn does_not_run_create_config() {
        // create-config would collide with the existing @snapshots subvolume.
        let actions = dry_actions(&Snapshots, &config());
        assert!(!actions.iter().any(|a| a.contains("create-config")));
    }

    #[test]
    fn locks_down_the_snapshots_directory() {
        let actions = dry_actions(&Snapshots, &config());
        assert!(actions.iter().any(|a| a.contains("chmod 0750 /.snapshots")));
        assert!(
            actions
                .iter()
                .any(|a| a.contains("chown root:root /.snapshots"))
        );
    }

    #[test]
    fn root_config_is_root_only_with_no_timeline() {
        assert!(ROOT_CONFIG.contains("SUBVOLUME=\"/\""));
        assert!(ROOT_CONFIG.contains("ALLOW_GROUPS=\"wheel\""));
        assert!(ROOT_CONFIG.contains("TIMELINE_CREATE=\"no\""));
        assert!(!ROOT_CONFIG.contains("/home"));
    }
}
