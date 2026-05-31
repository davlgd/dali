//! Step — prepare the live system's pacman before `pacstrap`: enable parallel
//! downloads (and friends) so the install itself is faster. The same tuning is
//! applied to the target's `pacman.conf` (in the base step) so it persists.

use super::{Context, Step};
use crate::error::Result;
use crate::system::probe;

/// Tunes the host pacman config (and, later, refreshes mirrors) before install.
pub struct HostPrep;

impl Step for HostPrep {
    fn name(&self) -> &'static str {
        "Prepare host pacman (config + mirrors)"
    }

    fn run(&self, ctx: &mut Context<'_>) -> Result<()> {
        if let Some(body) = probe::read_file("/etc/pacman.conf") {
            ctx.info("tuning pacman.conf (parallel downloads, color)");
            ctx.sys
                .write("/etc/pacman.conf", &tune_pacman_conf(&body))?;
        }
        Ok(())
    }
}

/// Settings enabled under `[options]`: (key, desired line).
const SETTINGS: [(&str, &str); 3] = [
    ("Color", "Color"),
    ("ParallelDownloads", "ParallelDownloads = 5"),
    ("VerbosePkgLists", "VerbosePkgLists"),
];

/// Enable `Color`, `ParallelDownloads = 5` and `VerbosePkgLists` under the
/// `[options]` section of a `pacman.conf` body, whether they were commented,
/// already set, or missing. Idempotent; other sections are left untouched.
pub(crate) fn tune_pacman_conf(body: &str) -> String {
    let mut out: Vec<String> = Vec::with_capacity(body.lines().count() + SETTINGS.len());
    let mut applied = [false; SETTINGS.len()];
    let mut in_options = false;

    for raw in body.lines() {
        let trimmed = raw.trim();
        if trimmed.starts_with('[') {
            if in_options {
                flush_missing(&mut out, &mut applied);
            }
            in_options = trimmed == "[options]";
            out.push(raw.to_owned());
            continue;
        }
        if in_options {
            let bare = trimmed.trim_start_matches('#').trim();
            if let Some(i) = SETTINGS.iter().position(|(key, _)| {
                bare == *key
                    || bare.starts_with(&format!("{key} "))
                    || bare.starts_with(&format!("{key}="))
            }) {
                if !applied[i] {
                    out.push(SETTINGS[i].1.to_owned());
                    applied[i] = true;
                }
                continue; // drop the commented original / any duplicate
            }
        }
        out.push(raw.to_owned());
    }
    if in_options {
        flush_missing(&mut out, &mut applied);
    }

    let mut result = out.join("\n");
    if body.ends_with('\n') {
        result.push('\n');
    }
    result
}

/// Append any not-yet-applied settings at the end of the `[options]` section.
fn flush_missing(out: &mut Vec<String>, applied: &mut [bool; SETTINGS.len()]) {
    for (i, done) in applied.iter_mut().enumerate() {
        if !*done {
            out.push(SETTINGS[i].1.to_owned());
            *done = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uncomments_the_known_options() {
        let input = "[options]\n#Color\n#ParallelDownloads = 5\n#VerbosePkgLists\n[core]\n";
        let out = tune_pacman_conf(input);
        assert!(out.contains("\nColor\n"));
        assert!(out.contains("\nParallelDownloads = 5\n"));
        assert!(out.contains("\nVerbosePkgLists\n"));
        assert!(!out.contains("#Color"));
        // The setting lines stay inside [options], before [core].
        assert!(out.find("Color").unwrap() < out.find("[core]").unwrap());
    }

    #[test]
    fn inserts_settings_when_absent() {
        let out = tune_pacman_conf("[options]\nHoldPkg = pacman glibc\n");
        assert!(out.contains("ParallelDownloads = 5"));
        assert!(out.contains("Color"));
        assert!(out.contains("VerbosePkgLists"));
    }

    #[test]
    fn is_idempotent() {
        let input = "[options]\n#Color\nParallelDownloads = 8\n[core]\n";
        let once = tune_pacman_conf(input);
        assert_eq!(tune_pacman_conf(&once), once);
        // An existing value is normalised to our setting, exactly once.
        assert_eq!(once.matches("ParallelDownloads").count(), 1);
    }

    #[test]
    fn dry_run_tunes_the_host_pacman_conf() {
        use crate::steps::test_support::{config, dry_actions};
        // On Arch (CI + dev host) /etc/pacman.conf exists, so a write is planned.
        let actions = dry_actions(&HostPrep, &config());
        assert!(
            actions
                .iter()
                .any(|a| a.contains("write: /etc/pacman.conf"))
        );
    }
}
