//! Step — set the root password (or lock it) and create the sudo-enabled
//! administrator account.

use super::{Context, Step, write_sudoers};
use crate::error::Result;
use crate::system::Command;

/// Configures the root account and creates the primary user.
pub struct Users;

impl Step for Users {
    fn name(&self) -> &'static str {
        "Create users"
    }

    fn run(&self, ctx: &mut Context<'_>) -> Result<()> {
        let config = ctx.config;

        if config.root_password.is_empty() {
            ctx.info("locking root account (administration via sudo)");
            ctx.sys
                .run(&Command::new("passwd").arg("--lock").arg("root").in_chroot())?;
        } else {
            ctx.info("setting root password");
            set_password(ctx, "root", config.root_password.expose())?;
        }

        // The `docker` group (created by the docker package) lets the user run
        // docker without sudo; only meaningful when the app set is installed.
        let groups = if config.default_apps {
            "wheel,docker"
        } else {
            "wheel"
        };
        let username = &config.user.username;
        ctx.info(format!("creating user {username} (groups {groups})"));
        ctx.sys.run(
            &Command::new("useradd")
                .arg("--create-home")
                .arg("--groups")
                .arg(groups)
                .arg("--shell")
                .arg("/bin/bash")
                .arg(username)
                .in_chroot(),
        )?;
        set_password(ctx, username, config.user.password.expose())?;

        // Grant the wheel group sudo. A drop-in keeps the main sudoers pristine.
        ctx.info("granting sudo to the wheel group");
        write_sudoers(ctx, "/etc/sudoers.d/10-wheel", "%wheel ALL=(ALL:ALL) ALL\n")?;
        Ok(())
    }
}

/// Set `account`'s password by piping `account:password` to `chpasswd`.
fn set_password(ctx: &mut Context<'_>, account: &str, password: &str) -> Result<()> {
    ctx.sys.run(
        &Command::new("chpasswd")
            .in_chroot()
            .stdin(format!("{account}:{password}\n")),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Secret;
    use crate::steps::test_support::{config, dry_actions};

    fn useradd_line(actions: &[String]) -> String {
        actions
            .iter()
            .find(|a| a.contains("useradd"))
            .expect("a useradd command is issued")
            .clone()
    }

    #[test]
    fn docker_group_is_added_only_with_the_default_app_set() {
        let mut cfg = config();

        cfg.default_apps = true;
        assert!(useradd_line(&dry_actions(&Users, &cfg)).contains("--groups wheel,docker"));

        cfg.default_apps = false;
        let line = useradd_line(&dry_actions(&Users, &cfg));
        assert!(line.contains("--groups wheel "), "wheel only: {line}");
        assert!(!line.contains("docker"), "no docker group: {line}");
    }

    #[test]
    fn empty_root_password_locks_root_instead_of_setting_one() {
        let mut cfg = config();

        cfg.root_password = Secret::new("");
        let actions = dry_actions(&Users, &cfg);
        assert!(actions.iter().any(|a| a.contains("passwd --lock root")));

        cfg.root_password = Secret::new("rootpw");
        let actions = dry_actions(&Users, &cfg);
        assert!(
            !actions.iter().any(|a| a.contains("passwd --lock")),
            "root with a password must not be locked"
        );
    }
}
