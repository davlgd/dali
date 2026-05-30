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
        ctx.info(format!(
            "appending aliases and helpers to /home/{user}/.bashrc"
        ));
        ctx.sys
            .append(&target_path(&format!("/home/{user}/.bashrc")), BASHRC_BLOCK)
    }
}

/// The block appended to `~/.bashrc`. Clipboard helpers shim macOS `pbcopy`/
/// `pbpaste` onto `wl-clipboard` / `xclip` / `xsel`.
const BASHRC_BLOCK: &str = r#"
# >>> DALI shell setup >>>
export PATH="$HOME/.local/bin:$PATH"

# macOS pbcopy/pbpaste → Wayland/X11 clipboard
pbcopy() {
    if command -v wl-copy >/dev/null; then wl-copy
    elif command -v xclip >/dev/null; then xclip -selection clipboard
    elif command -v xsel >/dev/null; then xsel --clipboard --input
    else cat >/dev/null; echo "pbcopy: no clipboard tool found" >&2; fi
}
pbpaste() {
    if command -v wl-paste >/dev/null; then wl-paste --no-newline
    elif command -v xclip >/dev/null; then xclip -selection clipboard -o
    elif command -v xsel >/dev/null; then xsel --clipboard --output
    else echo "pbpaste: no clipboard tool found" >&2; fi
}

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
alias pgen='gpg --gen-random --armor 2 32 | pbcopy'
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
