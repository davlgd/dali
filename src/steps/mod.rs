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
mod initramfs;
mod localization;
mod partition;
mod provision;
mod services;
mod storage;
mod users;

use crate::config::InstallConfig;
use crate::error::Result;
use crate::report::Reporter;
use crate::system::Sys;

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
        Box::new(base::Pacstrap),
        Box::new(fstab::GenerateFstab),
        Box::new(localization::Localization),
        Box::new(initramfs::Initramfs),
        Box::new(bootloader::Bootloader),
        Box::new(users::Users),
        Box::new(services::Services),
        Box::new(provision::Provision),
    ]
}

/// Run the whole pipeline against `config`, performing effects through `sys`
/// and reporting progress through `reporter`.
///
/// The configuration is validated first; a destructive step never runs on an
/// invalid config.
pub fn install(config: &InstallConfig, sys: &dyn Sys, reporter: &mut dyn Reporter) -> Result<()> {
    config.validate()?;

    let steps = pipeline();
    let total = steps.len();
    for (i, step) in steps.iter().enumerate() {
        reporter.step_start(i + 1, total, step.name());
        let mut ctx = Context {
            config,
            sys,
            reporter,
        };
        step.run(&mut ctx)?;
        reporter.step_done(step.name());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Secret, UserAccount};
    use crate::system::DrySys;

    struct NullReporter;
    impl Reporter for NullReporter {
        fn step_start(&mut self, _: usize, _: usize, _: &str) {}
        fn info(&mut self, _: &str) {}
        fn step_done(&mut self, _: &str) {}
    }

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
                "Install base system",
                "Generate fstab",
                "Configure localization",
                "Build initramfs",
                "Install bootloader",
                "Create users",
                "Enable services",
                "Provision extras (AUR, mise, Claude Code)",
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
