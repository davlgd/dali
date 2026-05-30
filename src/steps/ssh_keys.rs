//! Step — import a GitHub account's public keys as the user's accepted SSH
//! keys (`~/.ssh/authorized_keys`).
//!
//! Runs only when `github_user` is set. Best-effort: a fetch failure is
//! reported and skipped rather than aborting the install. The keys are public,
//! fetched from `https://github.com/<user>.keys`.

use super::{Context, Step};
use crate::error::Result;
use crate::system::{Command, target_path};

/// Imports `https://github.com/<github_user>.keys` into the user's authorized keys.
pub struct ImportSshKeys;

impl Step for ImportSshKeys {
    fn name(&self) -> &'static str {
        "Import GitHub SSH keys"
    }

    fn run(&self, ctx: &mut Context<'_>) -> Result<()> {
        let github_user = ctx.config.github_user.clone();
        if github_user.is_empty() {
            ctx.info("skipped (no GitHub user set)");
            return Ok(());
        }
        let user = ctx.config.user.username.clone();
        let url = format!("https://github.com/{github_user}.keys");

        ctx.info(format!("fetching SSH keys from {url}"));
        let keys = match ctx
            .sys
            .capture(&Command::new("curl").arg("-fsSL").arg(&url))
        {
            Ok(keys) => keys,
            Err(e) => {
                ctx.info(format!("warning: could not fetch SSH keys (skipping): {e}"));
                return Ok(());
            }
        };
        if ctx.sys.is_real() && keys.trim().is_empty() {
            ctx.info(format!(
                "warning: no public keys found for {github_user}; skipping"
            ));
            return Ok(());
        }

        let ssh_dir = target_path(&format!("/home/{user}/.ssh"));
        ctx.sys.mkdir_p(&ssh_dir)?;
        ctx.sys
            .write(&format!("{ssh_dir}/authorized_keys"), &keys)?;

        // sshd refuses keys it does not trust: the tree must be owned by the
        // user and not group/world-writable.
        let home_ssh = format!("/home/{user}/.ssh");
        ctx.sys.run(
            &Command::new("chown")
                .arg("-R")
                .arg(format!("{user}:{user}"))
                .arg(&home_ssh)
                .in_chroot(),
        )?;
        ctx.sys
            .run(&Command::new("chmod").arg("700").arg(&home_ssh).in_chroot())?;
        ctx.sys.run(
            &Command::new("chmod")
                .arg("600")
                .arg(format!("{home_ssh}/authorized_keys"))
                .in_chroot(),
        )?;
        Ok(())
    }
}
