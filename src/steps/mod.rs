//! The installation pipeline: an ordered list of small, single-purpose
//! [`Step`]s.
//!
//! Each step does one thing (Single-Responsibility) and depends only on the
//! [`Context`] — config plus the [`Sys`] effects boundary plus a
//! [`Reporter`]. Adding or reordering steps is a one-line change to
//! [`pipeline`], which is the Open/Closed payoff of this shape.

mod base;
mod bootloader;
mod fstab;
mod hardening;
mod host_pacman;
mod initramfs;
mod localization;
mod login_banner;
mod network;
mod partition;
mod provenance;
mod provision;
mod services;
mod shell;
mod snapshots;
mod ssh_keys;
mod storage;
mod tuning;
mod users;

use std::fmt::Write as _;

use crate::config::InstallConfig;
use crate::error::Result;
use crate::report::{Reporter, TranscriptReporter};
use crate::system::{Command, Sys, target_path};

/// Everything a step needs to do its job.
pub struct Context<'a> {
    /// The validated installation configuration.
    pub config: &'a InstallConfig,
    /// The effects boundary (real or dry-run).
    pub sys: &'a dyn Sys,
    /// Progress sink.
    pub reporter: &'a mut dyn Reporter,
}

impl Context<'_> {
    /// Convenience: report an informational line.
    fn info(&mut self, message: impl AsRef<str>) {
        self.reporter.info(message.as_ref());
    }
}

/// Write a sudoers drop-in at target path `name` with `contents`, then
/// `chmod 0440` it inside the chroot — the mode `sudo` requires for drop-ins
/// (it silently ignores group/world-writable ones). Shared so the mode lives
/// in one place across the steps that grant sudo.
fn write_sudoers(ctx: &Context<'_>, name: &str, contents: &str) -> Result<()> {
    ctx.sys.write(&target_path(name), contents)?;
    ctx.sys
        .run(&Command::new("chmod").arg("0440").arg(name).in_chroot())
}

/// A single, idempotent-as-possible unit of installation work.
pub trait Step {
    /// Short human-readable name shown in progress output.
    fn name(&self) -> &'static str;
    /// Perform the step.
    fn run(&self, ctx: &mut Context<'_>) -> Result<()>;
}

/// The ordered pipeline of steps for an opinionated minimal install.
pub fn pipeline() -> Vec<Box<dyn Step>> {
    vec![
        Box::new(partition::Partition),
        Box::new(storage::FormatAndMount),
        Box::new(host_pacman::HostPrep),
        Box::new(base::Pacstrap),
        Box::new(fstab::GenerateFstab),
        Box::new(localization::Localization),
        Box::new(network::CarryNetwork),
        Box::new(provenance::Provenance),
        Box::new(login_banner::LoginBanner),
        Box::new(initramfs::Initramfs),
        Box::new(bootloader::Bootloader),
        Box::new(users::Users),
        Box::new(shell::ShellSetup),
        Box::new(ssh_keys::ImportSshKeys),
        Box::new(services::Services),
        Box::new(snapshots::Snapshots),
        Box::new(tuning::Tuning),
        Box::new(hardening::Harden),
        Box::new(provision::Provision),
    ]
}

/// Run the whole pipeline against `config`, performing effects through `sys`
/// and reporting progress through `reporter`.
///
/// The configuration is validated first; a destructive step never runs on an
/// invalid config.
pub fn install(config: &InstallConfig, sys: &dyn Sys, reporter: &mut dyn Reporter) -> Result<()> {
    install_with(&pipeline(), config, sys, reporter)
}

/// Run a specific list of `steps` as a full install: validate, run the pipeline,
/// then persist the transcript and per-step status into the target regardless of
/// outcome. Split out from [`install`] so tests can drive a custom pipeline
/// (e.g. inject a failing step) without rebuilding the diagnostics plumbing.
fn install_with(
    steps: &[Box<dyn Step>],
    config: &InstallConfig,
    sys: &dyn Sys,
    reporter: &mut dyn Reporter,
) -> Result<()> {
    config.validate()?;

    // Capture a transcript + per-step status alongside the live reporter so they
    // can be written into the installed system.
    let mut transcript = TranscriptReporter::new(reporter);
    let outcome = run_pipeline(steps, config, sys, &mut transcript);

    // Best-effort diagnostics, so they never mask the real outcome and so a
    // failed install still records how far it got (where /mnt is available).
    let _ = write_install_log(sys, &transcript);
    let _ = write_step_status(sys, &transcript);
    outcome
}

/// Run each step in order, reporting through `reporter`.
fn run_pipeline(
    steps: &[Box<dyn Step>],
    config: &InstallConfig,
    sys: &dyn Sys,
    reporter: &mut dyn Reporter,
) -> Result<()> {
    let total = steps.len();
    for (i, step) in steps.iter().enumerate() {
        reporter.step_start(i + 1, total, step.name());
        let result = {
            let mut ctx = Context {
                config,
                sys,
                reporter,
            };
            step.run(&mut ctx)
        };
        match result {
            Ok(()) => reporter.step_done(step.name()),
            Err(e) => {
                // Record the reason in the transcript before unwinding, so the
                // persisted install log explains why a partial install stopped.
                reporter.info(&format!("step failed: {e}"));
                return Err(e);
            }
        }
    }
    Ok(())
}

/// Persist the install transcript into the target as `/var/log/dali-install.log`.
fn write_install_log(sys: &dyn Sys, transcript: &TranscriptReporter) -> Result<()> {
    sys.mkdir_p(&target_path("/var/log"))?;
    sys.write(
        &target_path("/var/log/dali-install.log"),
        transcript.transcript(),
    )
}

/// Persist a per-step completion map into the target as
/// `/var/log/dali-steps.toml`, so a partial install is diagnosable.
fn write_step_status(sys: &dyn Sys, transcript: &TranscriptReporter) -> Result<()> {
    sys.mkdir_p(&target_path("/var/log"))?;
    sys.write(
        &target_path("/var/log/dali-steps.toml"),
        &step_status_toml(transcript.steps()),
    )
}

/// Render step records as TOML (`[[step]]` tables).
fn step_status_toml(records: &[crate::report::StepRecord]) -> String {
    let mut out = String::new();
    for record in records {
        let _ = writeln!(
            out,
            "[[step]]\nindex = {}\ntotal = {}\nname = \"{}\"\ncompleted = {}\n",
            record.index, record.total, record.name, record.completed
        );
    }
    out
}

/// Shared test helpers for exercising individual steps against a dry-run `Sys`.
#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use crate::config::{Secret, UserAccount};
    use crate::system::DrySys;

    /// A reporter that swallows progress output.
    pub(crate) struct NullReporter;
    impl Reporter for NullReporter {
        fn step_start(&mut self, _: usize, _: usize, _: &str) {}
        fn info(&mut self, _: &str) {}
        fn step_done(&mut self, _: &str) {}
    }

    /// A minimal config with a named user, for driving a single step.
    pub(crate) fn config() -> InstallConfig {
        InstallConfig {
            disk: "/dev/vda".to_owned(),
            user: UserAccount {
                username: "alice".to_owned(),
                password: Secret::new("pw"),
            },
            ..InstallConfig::default()
        }
    }

    /// Run `step` against a dry-run `Sys` and return the actions it recorded.
    pub(crate) fn dry_actions(step: &dyn Step, config: &InstallConfig) -> Vec<String> {
        let sys = DrySys::new();
        let mut reporter = NullReporter;
        let mut ctx = Context {
            config,
            sys: &sys,
            reporter: &mut reporter,
        };
        step.run(&mut ctx).unwrap();
        sys.actions()
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::NullReporter;
    use super::*;
    use crate::config::{Secret, UserAccount};
    use crate::system::DrySys;

    fn sample_config() -> InstallConfig {
        InstallConfig {
            disk: "/dev/vda".into(),
            user: UserAccount {
                username: "arch".into(),
                password: Secret::new("pw"),
            },
            root_password: Secret::new("rootpw"),
            ..InstallConfig::default()
        }
    }

    #[test]
    fn pipeline_has_expected_steps_in_order() {
        let names: Vec<_> = pipeline().iter().map(|s| s.name()).collect();
        assert_eq!(
            names,
            [
                "Partition disk",
                "Create filesystems and mount",
                "Prepare host pacman (config + mirrors)",
                "Install base system",
                "Generate fstab",
                "Configure localization",
                "Carry network configuration",
                "Write provenance marker",
                "Configure login banner",
                "Build initramfs",
                "Install bootloader",
                "Create users",
                "Configure shell environment",
                "Import GitHub SSH keys",
                "Enable services",
                "Configure snapshots (snapper)",
                "Apply system tuning",
                "Harden the system (sshd, firewall)",
                "Provision extras (V, mise, Claude Code)",
            ]
        );
    }

    #[test]
    fn full_dry_run_succeeds_and_records_actions() {
        let config = sample_config();
        let sys = DrySys::new();
        let mut reporter = NullReporter;
        install(&config, &sys, &mut reporter).unwrap();

        let actions = sys.actions();
        assert!(actions.iter().any(|a| a.contains("sgdisk")));
        assert!(actions.iter().any(|a| a.contains("pacstrap")));
        assert!(actions.iter().any(|a| a.contains("bootctl")));
        assert!(actions.iter().any(|a| a.contains("useradd")));
        assert!(
            actions
                .iter()
                .any(|a| a.contains("/var/log/dali-install.log")),
            "the install transcript is written into the target"
        );
        assert!(
            actions
                .iter()
                .any(|a| a.contains("/var/log/dali-steps.toml")),
            "the per-step status is written into the target"
        );
    }

    #[test]
    fn step_status_toml_renders_completion() {
        use crate::report::StepRecord;
        let toml = step_status_toml(&[
            StepRecord {
                index: 1,
                total: 2,
                name: "Partition disk".to_owned(),
                completed: true,
            },
            StepRecord {
                index: 2,
                total: 2,
                name: "Install base system".to_owned(),
                completed: false,
            },
        ]);
        assert!(toml.contains("name = \"Partition disk\""));
        assert!(toml.contains("total = 2"));
        assert!(toml.contains("completed = true"));
        assert!(toml.contains("completed = false"));
    }

    #[test]
    fn pipeline_records_the_failure_reason_in_the_transcript() {
        use crate::report::TranscriptReporter;

        struct Boom;
        impl Step for Boom {
            fn name(&self) -> &'static str {
                "Boom"
            }
            fn run(&self, _: &mut Context<'_>) -> Result<()> {
                Err(crate::error::Error::Config("kaboom".into()))
            }
        }

        let steps: Vec<Box<dyn Step>> = vec![Box::new(Boom)];
        let sys = DrySys::new();
        let mut inner = NullReporter;
        let mut transcript = TranscriptReporter::new(&mut inner);
        let result = run_pipeline(&steps, &sample_config(), &sys, &mut transcript);

        assert!(result.is_err());
        assert!(transcript.transcript().contains("step failed"));
        assert!(transcript.transcript().contains("kaboom"));
    }

    #[test]
    fn install_persists_diagnostics_even_when_a_step_fails() {
        // A mid-pipeline failure must still propagate the error *and* leave the
        // install log + step status behind (they explain a partial install).
        struct Ok1;
        impl Step for Ok1 {
            fn name(&self) -> &'static str {
                "Ok step"
            }
            fn run(&self, ctx: &mut Context<'_>) -> Result<()> {
                ctx.info("did a thing");
                Ok(())
            }
        }
        struct Boom;
        impl Step for Boom {
            fn name(&self) -> &'static str {
                "Boom"
            }
            fn run(&self, _: &mut Context<'_>) -> Result<()> {
                Err(crate::error::Error::Config("kaboom".into()))
            }
        }

        let steps: Vec<Box<dyn Step>> = vec![Box::new(Ok1), Box::new(Boom)];
        let sys = DrySys::new();
        let mut reporter = NullReporter;
        let result = install_with(&steps, &sample_config(), &sys, &mut reporter);

        assert!(result.is_err(), "the failure propagates to the caller");
        let actions = sys.actions();
        assert!(
            actions
                .iter()
                .any(|a| a.contains("/var/log/dali-install.log")),
            "the install log is persisted on failure"
        );
        assert!(
            actions
                .iter()
                .any(|a| a.contains("/var/log/dali-steps.toml")),
            "the per-step status is persisted on failure"
        );
    }

    #[test]
    fn install_refuses_invalid_config() {
        let config = InstallConfig::default(); // no disk, no password
        let sys = DrySys::new();
        let mut reporter = NullReporter;
        assert!(install(&config, &sys, &mut reporter).is_err());
        assert!(sys.actions().is_empty(), "no effects on invalid config");
    }
}
