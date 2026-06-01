//! Step — append the opinionated shell environment (aliases, helper functions,
//! PATH) to the primary user's `~/.bashrc`, and put `~/.local/bin` on `PATH`
//! system-wide via `/etc/profile.d`.
//!
//! Deterministic and offline, so it always runs (independent of `provision`).

use super::{Context, Step};
use crate::error::Result;
use crate::system::{Command, target_path};

/// Writes the DALI shell setup into the user's bash configuration.
pub struct ShellSetup;

impl Step for ShellSetup {
    fn name(&self) -> &'static str {
        "Configure shell environment"
    }

    fn run(&self, ctx: &mut Context<'_>) -> Result<()> {
        let user = &ctx.config.user.username;

        // Put ~/.local/bin on PATH for every login shell (system-wide), so the
        // tools installed there during provisioning — Claude Code, mise, the V
        // symlink — are found both by the provisioning login shells and after
        // reboot. profile.d is the canonical, shell-agnostic place for this.
        ctx.info("adding ~/.local/bin to PATH (/etc/profile.d)");
        ctx.sys
            .write(&target_path("/etc/profile.d/10-dali-path.sh"), PROFILE_PATH)?;

        // The PATH addition above is infrastructure (tools installed during
        // provisioning must be found); the alias/helper block is opt-out.
        if !ctx.config.shell.aliases {
            ctx.info("skipping the ~/.bashrc alias block (shell.aliases = false)");
            return Ok(());
        }

        ctx.info(format!(
            "writing aliases and helpers to /home/{user}/.bashrc"
        ));
        // Marker-delimited so re-running replaces the block instead of appending
        // a duplicate.
        ctx.sys.write_block(
            &target_path(&format!("/home/{user}/.bashrc")),
            BASHRC_BEGIN,
            BASHRC_END,
            BASHRC_BLOCK,
        )?;

        // write_block ran as root, so `.bashrc` (and any `.dali.bak` backup) are
        // now root-owned; hand the home tree back so the user can edit their own
        // config. (Only those files changed owner; the rest is already theirs.)
        ctx.sys.run(
            &Command::new("chown")
                .arg("-R")
                .arg(format!("{user}:{user}"))
                .arg(format!("/home/{user}"))
                .in_chroot(),
        )
    }
}

/// Markers delimiting the DALI block in `~/.bashrc` (must match [`BASHRC_BLOCK`]).
const BASHRC_BEGIN: &str = "# >>> DALI shell setup >>>";
const BASHRC_END: &str = "# <<< DALI shell setup <<<";

/// System-wide PATH addition for login shells; idempotent (no duplicate entry).
const PROFILE_PATH: &str = r#"# Added by DALI: per-user local binaries on PATH
case ":$PATH:" in
    *":$HOME/.local/bin:"*) ;;
    *) export PATH="$HOME/.local/bin:$PATH" ;;
esac
"#;

/// The block appended to `~/.bashrc`. Functions and aliases are kept in
/// alphabetical order.
const BASHRC_BLOCK: &str = r#"
# >>> DALI shell setup >>>
case ":$PATH:" in *":$HOME/.local/bin:"*) ;; *) export PATH="$HOME/.local/bin:$PATH" ;; esac

# Activate mise (runtime/tool manager) when installed.
command -v mise >/dev/null && eval "$(mise activate bash)"

# Atuin shell history — needs bash-preexec for bash; both are guarded so this is
# a no-op if the curated app set was not installed.
if command -v atuin >/dev/null; then
    [ -f /usr/share/bash-preexec/bash-preexec.sh ] && . /usr/share/bash-preexec/bash-preexec.sh
    eval "$(atuin init bash)"
fi

check() {
    if curl --output /dev/null --silent --head --fail "$1"; then
        echo "$1 is online"
    else
        echo "$1 is offline"
    fi
}

clean_cargo() {
  pushd ~
  find Documents -type f -name Cargo.toml -exec cargo clean --manifest-path {} \; 2>&1 | grep Removed
  popd
}

f() { find / -type f -name "$1" 2> /dev/null; }

mkcd() { mkdir -p -- "$1" && cd -- "$1"; }

up() {
    sudo pacman -Syu
    if command -v mise >/dev/null; then mise self-update -y || true; mise upgrade; fi
    command -v bun >/dev/null && bun update -g
    command -v uv >/dev/null && uv tool upgrade --all
    command -v v >/dev/null && v up
}

w() {
    if [[ "$2" == "--full" ]]; then
        curl "wttr.in/${1}"
    else
        curl "wttr.in/${1}?format=2"
    fi
}

alias add='sudo pacman -S'
alias dps='docker ps --format "table {{.Names}}\t{{.Status}}\t{{.Ports}}"'
alias gac='git add . && git commit -m'
alias gl='git log --oneline --all --graph --decorate'
alias gri='git rebase -i'
alias grroot='git rebase -i --root'
alias gst='git status'
alias gsw='git switch'
alias list='pacman -Qe'
alias myip='curl -s monip.org | sed "s/</\n</g" | sed -n "s/.*IP : \([0-9.]*\).*/IP: \1/p; s/<i>\(.*\)/Reverse: \1/p"'
alias pgen='gpg --gen-random --armor 2 32'
alias remove='sudo pacman -Rns'
alias search='pacman -Ss'
# <<< DALI shell setup <<<
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::steps::test_support::{config, dry_actions};

    #[test]
    fn markers_match_the_block() {
        assert!(BASHRC_BLOCK.contains(BASHRC_BEGIN));
        assert!(BASHRC_BLOCK.contains(BASHRC_END));
    }

    #[test]
    fn writes_profile_path_and_bashrc_block() {
        let actions = dry_actions(&ShellSetup, &config());
        assert!(
            actions
                .iter()
                .any(|a| a.contains("write: /mnt/etc/profile.d/10-dali-path.sh"))
        );
        assert!(
            actions
                .iter()
                .any(|a| a.contains("write_block: /mnt/home/alice/.bashrc"))
        );
        // The bashrc is written as root, then handed back to the user.
        assert!(
            actions
                .iter()
                .any(|a| a.contains("chown -R alice:alice /home/alice"))
        );
    }

    #[test]
    fn disabling_aliases_keeps_path_but_skips_the_bashrc_block() {
        let mut cfg = config();
        cfg.shell.aliases = false;
        let actions = dry_actions(&ShellSetup, &cfg);
        // PATH is infrastructure and still written.
        assert!(
            actions
                .iter()
                .any(|a| a.contains("/etc/profile.d/10-dali-path.sh"))
        );
        // The alias block and its chown are skipped.
        assert!(!actions.iter().any(|a| a.contains(".bashrc")));
        assert!(!actions.iter().any(|a| a.contains("chown")));
    }
}
