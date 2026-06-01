//! Step — prepare the live system's pacman before `pacstrap`: enable parallel
//! downloads (and friends) so the install itself is faster. The same tuning is
//! applied to the target's `pacman.conf` (in the base step) so it persists.

use super::{Context, Step};
use crate::error::Result;
use crate::system::{Command, probe};

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
        refresh_mirrors(ctx);
        Ok(())
    }
}

/// Rank mirrors with reflector by speed, optionally restricted to the
/// configured `mirror_country` (worldwide by default), falling back to a
/// worldwide ranking, then to the stock mirrorlist. Best-effort — a missing
/// reflector or no network must never abort the install.
fn refresh_mirrors(ctx: &mut Context<'_>) {
    let configured = ctx.config.mirror_country.trim();
    let country = (!configured.is_empty()).then_some(configured);
    ctx.info("refreshing the mirrorlist (reflector)");
    if ctx.sys.run(&reflector_cmd(country)).is_err() {
        let recovered = country.is_some() && ctx.sys.run(&reflector_cmd(None)).is_ok();
        if !recovered {
            ctx.info("reflector unavailable; keeping the existing mirrorlist");
        }
    }
}

/// `reflector` command ranking mirrors by speed over HTTPS, optionally filtered
/// to `country`, saving over the live mirrorlist (pacstrap propagates it).
fn reflector_cmd(country: Option<&str>) -> Command {
    let mut cmd = Command::new("reflector");
    if let Some(cc) = country {
        cmd = cmd.arg("--country").arg(cc);
    }
    cmd.arg("--protocol")
        .arg("https")
        .arg("--latest")
        .arg("20")
        .arg("--sort")
        .arg("rate")
        .arg("--save")
        .arg("/etc/pacman.d/mirrorlist")
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
    let mut saw_options = false;

    for raw in body.lines() {
        let trimmed = raw.trim();
        if trimmed.starts_with('[') {
            if in_options {
                flush_missing(&mut out, &mut applied);
            }
            in_options = trimmed == "[options]";
            saw_options |= in_options;
            out.push(raw.to_owned());
            continue;
        }
        if in_options {
            let bare = trimmed.trim_start_matches('#').trim();
            if let Some(i) = SETTINGS.iter().position(|(key, _)| {
                bare == *key
                    || bare
                        .strip_prefix(key)
                        .is_some_and(|rest| rest.starts_with(' ') || rest.starts_with('='))
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
    } else if !saw_options {
        // No `[options]` section at all — add one so the settings still apply.
        out.push("[options]".to_owned());
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
    fn adds_an_options_section_when_none_exists() {
        let out = tune_pacman_conf("[core]\nInclude = /etc/pacman.d/mirrorlist\n");
        assert!(out.contains("[options]"));
        assert!(out.contains("ParallelDownloads = 5"));
        // The original section is preserved.
        assert!(out.contains("[core]"));
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

    #[test]
    fn reflector_cmd_filters_by_country_and_sorts_by_rate() {
        assert_eq!(
            reflector_cmd(Some("FR")).to_string(),
            "reflector --country FR --protocol https --latest 20 --sort rate --save /etc/pacman.d/mirrorlist"
        );
    }

    #[test]
    fn reflector_cmd_without_country_is_worldwide() {
        let cmd = reflector_cmd(None).to_string();
        assert!(!cmd.contains("--country"));
        assert!(cmd.contains("--sort rate"));
    }

    #[test]
    fn dry_run_refreshes_mirrors_after_tuning() {
        use crate::config::InstallConfig;
        use crate::steps::test_support::{config, dry_actions};

        let fr = InstallConfig {
            mirror_country: "FR".to_owned(),
            ..config()
        };
        let actions = dry_actions(&HostPrep, &fr);
        let tune = actions
            .iter()
            .position(|a| a.contains("write: /etc/pacman.conf"))
            .expect("pacman.conf tuned");
        let reflector = actions
            .iter()
            .position(|a| a.contains("reflector --country FR"))
            .expect("reflector ran for FR");
        assert!(tune < reflector, "mirrors refreshed after tuning");
    }

    #[test]
    fn dry_run_default_ranks_mirrors_worldwide() {
        use crate::steps::test_support::{config, dry_actions};
        // No mirror_country configured -> reflector runs without --country.
        let actions = dry_actions(&HostPrep, &config());
        assert!(
            actions
                .iter()
                .any(|a| a.contains("reflector") && !a.contains("--country"))
        );
    }
}
