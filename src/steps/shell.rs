//! Step — append the opinionated shell environment (aliases, helper functions,
//! PATH) to the primary user's `~/.bashrc`.
//!
//! Deterministic and offline, so it always runs (independent of `provision`).
//! macOS-isms from the source dotfile (`pbcopy`) are shimmed onto Linux
//! clipboard tools.

use super::{Context, Step};
use crate::error::Result;
use crate::system::target_path;

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

        ctx.info(format!(
            "appending aliases and helpers to /home/{user}/.bashrc"
        ));
        ctx.sys
            .append(&target_path(&format!("/home/{user}/.bashrc")), BASHRC_BLOCK)
    }
}

/// System-wide PATH addition for login shells; idempotent (no duplicate entry).
const PROFILE_PATH: &str = r#"# Added by DALI: per-user local binaries on PATH
case ":$PATH:" in
    *":$HOME/.local/bin:"*) ;;
    *) export PATH="$HOME/.local/bin:$PATH" ;;
esac
"#;

/// The block appended to `~/.bashrc`.
const BASHRC_BLOCK: &str = r#"
# >>> DALI shell setup >>>
case ":$PATH:" in *":$HOME/.local/bin:"*) ;; *) export PATH="$HOME/.local/bin:$PATH" ;; esac

mkcd() { mkdir -p -- "$1" && cd -- "$1"; }

check() {
    if curl --output /dev/null --silent --head --fail "$1"; then
        echo "$1 is online"
    else
        echo "$1 is offline"
    fi
}

w() {
    if [[ "$2" == "--full" ]]; then
        curl "wttr.in/${1}"
    else
        curl "wttr.in/${1}?format=2"
    fi
}

clean_cargo() {
  pushd ~
  find Documents -type f -name Cargo.toml -exec cargo clean --manifest-path {} \; 2>&1 | grep Removed
  popd
}

f() { find / -type f -name "$1" 2> /dev/null; }
alias pgen='gpg --gen-random --armor 2 32'
alias gl='git log --oneline --all --graph --decorate'
alias gac='git add . && git commit -m'
alias gst='git status'
alias gsw='git switch'
alias gri='git rebase -i'
alias grroot='git rebase -i --root'

alias myip='curl -s monip.org | sed "s/</\n</g" | sed -n "s/.*IP : \([0-9.]*\).*/IP: \1/p; s/<i>\(.*\)/Reverse: \1/p"'
alias dps='docker ps --format "table {{.Names}}\t{{.Status}}\t{{.Ports}}"'
# <<< DALI shell setup <<<
"#;
