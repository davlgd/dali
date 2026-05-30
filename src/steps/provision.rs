//! Step 10 — best-effort post-install provisioning: AUR packages plus the
//! `mise` and Claude Code installer scripts.
//!
//! This step is **best-effort and network-bound**: everything here runs as the
//! freshly created user inside the target, and any failure is reported as a
//! warning rather than aborting — the system is already bootable by this point.
//! It is skipped entirely when `provision` is false.

use super::{Context, Step};
use crate::error::Result;
use crate::system::{Command, target_path};

/// Sudoers drop-in granting passwordless sudo during provisioning only.
const NOPASSWD_DROPIN: &str = "/etc/sudoers.d/99-dali-provision";

/// Installs AUR packages (via a bootstrapped `paru`) and per-user tools.
pub struct Provision;

impl Step for Provision {
    fn name(&self) -> &'static str {
        "Provision extras (AUR, mise, Claude Code)"
    }

    fn run(&self, ctx: &mut Context<'_>) -> Result<()> {
        if !ctx.config.provision {
            ctx.info("skipped (provision disabled)");
            return Ok(());
        }
        let user = ctx.config.user.username.clone();

        // `arch-chroot` does not guarantee working DNS, so give the target a
        // public resolver for the duration. NetworkManager manages
        // /etc/resolv.conf itself after first boot, overwriting this.
        ctx.sys.write(
            &target_path("/etc/resolv.conf"),
            "nameserver 9.9.9.9\nnameserver 1.1.1.1\n",
        )?;

        // makepkg/paru must install build results without an interactive sudo
        // password; grant it temporarily and revoke it at the end.
        ctx.sys.write(
            &target_path(NOPASSWD_DROPIN),
            "%wheel ALL=(ALL:ALL) NOPASSWD: ALL\n",
        )?;
        ctx.sys.run(
            &Command::new("chmod")
                .arg("0440")
                .arg(NOPASSWD_DROPIN)
                .in_chroot(),
        )?;

        // 1. Bootstrap the paru AUR helper (prebuilt, no compile), then use it
        //    to resolve and install the AUR package set.
        best_effort(
            ctx,
            "bootstrapping the paru AUR helper",
            &user_sh(
                &user,
                "rm -rf /tmp/paru-bin && \
                 git clone --depth 1 https://aur.archlinux.org/paru-bin.git /tmp/paru-bin && \
                 cd /tmp/paru-bin && makepkg -si --noconfirm",
            ),
        );
        for pkg in ctx.config.aur_packages.clone() {
            best_effort(
                ctx,
                &format!("installing AUR package {pkg}"),
                &user_sh(&user, &format!("paru -S --noconfirm --skipreview {pkg}")),
            );
        }

        // 2. Build the V compiler from source and symlink it into ~/.local/bin.
        best_effort(
            ctx,
            "building the V compiler",
            &user_sh(
                &user,
                "mkdir -p ~/.local/bin && rm -rf ~/v && \
                 git clone --depth=1 https://github.com/vlang/v ~/v && \
                 cd ~/v && make && ./v symlink ~/.local/bin",
            ),
        );

        // 3. Per-user tool installers (write into the user's home).
        best_effort(
            ctx,
            "installing mise",
            &user_sh(&user, "curl -fsSL https://mise.run | sh"),
        );
        best_effort(
            ctx,
            "installing Claude Code",
            &user_sh(&user, "curl -fsSL https://claude.ai/install.sh | bash"),
        );

        // Revoke the passwordless sudo grant.
        ctx.sys.run(
            &Command::new("rm")
                .arg("-f")
                .arg(NOPASSWD_DROPIN)
                .in_chroot(),
        )?;
        Ok(())
    }
}

/// A command running `script` as `user` with a login shell inside the target.
fn user_sh(user: &str, script: &str) -> Command {
    Command::new("runuser")
        .arg("-u")
        .arg(user)
        .arg("--")
        .arg("bash")
        .arg("-lc")
        .arg(script)
        .in_chroot()
}

/// Run `command`, downgrading any failure to a warning so provisioning never
/// aborts the (already complete) install.
fn best_effort(ctx: &mut Context<'_>, what: &str, command: &Command) {
    ctx.info(what);
    if let Err(e) = ctx.sys.run(command) {
        ctx.info(format!("warning: {what} failed (continuing): {e}"));
    }
}
