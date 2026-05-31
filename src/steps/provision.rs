//! Step — best-effort post-install provisioning: the V compiler, the `mise`
//! and Claude Code installer scripts, and any user-supplied `custom_commands`.
//!
//! The installer scripts and `custom_commands` are **best-effort and
//! network-bound**: they run as the freshly created user inside the target, and
//! any failure is reported as a warning rather than aborting — the system is
//! already bootable by this point. (The sudo grant/revoke and its tamper-evident
//! check are not best-effort: they propagate errors.) Skipped when `provision`
//! is false.

use super::{Context, Step, write_sudoers};
use crate::config::stack;
use crate::error::Result;
use crate::system::{Command, target_path};

/// Sudoers drop-in granting passwordless sudo during provisioning only.
const NOPASSWD_DROPIN: &str = "/etc/sudoers.d/99-dali-provision";

/// Builds the V compiler and runs the per-user tool installers.
pub struct Provision;

impl Step for Provision {
    fn name(&self) -> &'static str {
        "Provision extras (V, mise, Claude Code)"
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

        // `custom_commands` may use sudo; grant the wheel group passwordless
        // sudo for the duration and revoke it at the end.
        write_sudoers(ctx, NOPASSWD_DROPIN, "%wheel ALL=(ALL:ALL) NOPASSWD: ALL\n")?;

        // 1. Build the V compiler from source and symlink it into ~/.local/bin.
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

        // 2. Per-user tool installers (write into the user's home).
        best_effort(
            ctx,
            "installing mise",
            &user_sh(&user, "curl -fsSL https://mise.run | sh"),
        );
        best_effort(
            ctx,
            "installing global tools via mise",
            &user_sh(
                &user,
                &format!(
                    "~/.local/bin/mise use -g {}",
                    stack::MISE_GLOBAL_TOOLS.join(" ")
                ),
            ),
        );
        best_effort(
            ctx,
            "installing Claude Code",
            &user_sh(&user, "curl -fsSL https://claude.ai/install.sh | bash"),
        );

        // 3. User-supplied commands, while passwordless sudo is still granted.
        for cmd in ctx.config.custom_commands.clone() {
            let cmd = cmd.trim();
            if cmd.is_empty() {
                continue;
            }
            best_effort(
                ctx,
                &format!("running custom command: {cmd}"),
                &user_sh(&user, cmd),
            );
        }

        // Revoke the temporary passwordless-sudo grant. Best-effort so a
        // transient `rm` failure doesn't abort an otherwise-complete install…
        best_effort(
            ctx,
            "revoking the provisioning sudo grant",
            &Command::new("rm")
                .arg("-f")
                .arg(NOPASSWD_DROPIN)
                .in_chroot(),
        );
        // …but then hard-fail if the drop-in somehow survived: leaving it would
        // be a wheel-wide passwordless-root backdoor. `test ! -e` exits non-zero
        // (→ error) when the file still exists.
        ctx.sys.run(
            &Command::new("test")
                .arg("!")
                .arg("-e")
                .arg(NOPASSWD_DROPIN)
                .in_chroot(),
        )
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::steps::test_support::{config, dry_actions};

    #[test]
    fn provision_builds_v_and_installs_user_tools() {
        let actions = dry_actions(&Provision, &config());
        assert!(actions.iter().any(|a| a.contains("vlang/v")));
        assert!(actions.iter().any(|a| a.contains("mise use -g")));
    }

    #[test]
    fn provision_disabled_does_nothing() {
        let mut cfg = config();
        cfg.provision = false;
        assert!(dry_actions(&Provision, &cfg).is_empty());
    }

    #[test]
    fn custom_commands_run_before_the_sudo_revoke() {
        let mut cfg = config();
        cfg.custom_commands = vec!["touch /tmp/marker".to_owned(), "  ".to_owned()];
        let actions = dry_actions(&Provision, &cfg);
        let cmd = actions
            .iter()
            .position(|a| a.contains("touch /tmp/marker"))
            .expect("custom command runs");
        let revoke = actions
            .iter()
            .position(|a| a.contains("rm -f /etc/sudoers.d/99-dali-provision"))
            .expect("sudo revoked");
        assert!(
            cmd < revoke,
            "custom commands run while sudo is still granted"
        );
    }
}
