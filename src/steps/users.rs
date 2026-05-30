//! Step 8 — set the root password (or lock it) and create the sudo-enabled
//! administrator account.

use super::{Context, Step};
use crate::error::Result;
use crate::system::{Command, target_path};

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

        let username = &config.user.username;
        ctx.info(format!("creating user {username} (group wheel)"));
        ctx.sys.run(
            &Command::new("useradd")
                .arg("--create-home")
                .arg("--groups")
                .arg("wheel")
                .arg("--shell")
                .arg("/bin/bash")
                .arg(username)
                .in_chroot(),
        )?;
        set_password(ctx, username, config.user.password.expose())?;

        // Grant the wheel group sudo. A drop-in keeps the main sudoers pristine.
        ctx.info("granting sudo to the wheel group");
        ctx.sys.write(
            &target_path("/etc/sudoers.d/10-wheel"),
            "%wheel ALL=(ALL:ALL) ALL\n",
        )?;
        ctx.sys.run(
            &Command::new("chmod")
                .arg("0440")
                .arg("/etc/sudoers.d/10-wheel")
                .in_chroot(),
        )?;
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
