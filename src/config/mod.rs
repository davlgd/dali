//! Installation configuration: the single source of truth describing *what*
//! DALI will install.
//!
//! DALI is **opinionated minimal**: the technical stack is fixed (UEFI + GPT,
//! Btrfs root, systemd-boot, the `linux` kernel, NetworkManager) and lives in
//! [`stack`]. The only things the user actually decides are captured in
//! [`InstallConfig`]. The [`Secret`] type and the input validators live in
//! their own submodules so this file stays focused on the config model.

pub mod stack;

mod secret;
mod validate;

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};
use validate::{validate_github_user, validate_hostname, validate_package_name, validate_username};

pub use secret::Secret;

/// The administrator account created during installation.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UserAccount {
    /// Login name.
    pub username: String,
    /// Account password.
    pub password: Secret,
}

/// Everything the user gets to decide. Sensible defaults are provided for all
/// non-destructive fields so a config can be partial.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct InstallConfig {
    /// Target block device to install onto, e.g. `/dev/sda` or `/dev/vda`.
    ///
    /// **This disk is wiped.** Empty means "not chosen yet".
    pub disk: String,
    /// System hostname.
    pub hostname: String,
    /// Timezone in `Region/City` form, e.g. `Europe/Paris`.
    pub timezone: String,
    /// Glibc locale, e.g. `en_US.UTF-8`.
    pub locale: String,
    /// Console keymap, e.g. `us` or `fr`.
    pub keymap: String,
    /// The administrator account to create (member of `wheel`, sudo-enabled).
    pub user: UserAccount,
    /// Root password. If empty, the root account is locked and administration
    /// happens exclusively through the sudo-enabled [`Self::user`].
    pub root_password: Secret,
    /// Optional GitHub username. When set, that account's public keys
    /// (`https://github.com/<user>.keys`) are imported as the user's accepted
    /// SSH keys. Empty means "don't import".
    pub github_user: String,
    /// Extra packages to install on top of [`stack::BASE_PACKAGES`].
    pub extra_packages: Vec<String>,
    /// AUR packages to install during provisioning via the bootstrapped `paru`.
    /// Empty by default — `pamac-aur` was dropped as it currently needs an
    /// older `libalpm` than Arch ships; add packages here once compatible.
    pub aur_packages: Vec<String>,
    /// Enable a compressed RAM swap device (zram) sized to total RAM, capped at
    /// 8 GiB.
    pub zram_swap: bool,
    /// Install the curated [`stack::DEFAULT_APPS`] set and enable their services
    /// ([`stack::APP_SERVICES`]: docker, avahi, sshd). Disable for a bare
    /// bootable system.
    pub default_apps: bool,
    /// Run the post-install provisioning: AUR packages (from
    /// [`Self::aur_packages`]) and the `mise` / Claude Code installers.
    /// Best-effort and network-bound.
    pub provision: bool,
}

impl Default for InstallConfig {
    fn default() -> Self {
        Self {
            disk: String::new(),
            hostname: "arch".to_owned(),
            timezone: "UTC".to_owned(),
            locale: "en_US.UTF-8".to_owned(),
            keymap: "us".to_owned(),
            user: UserAccount {
                // No default username on purpose — the user must choose one.
                username: String::new(),
                password: Secret::default(),
            },
            root_password: Secret::default(),
            github_user: String::new(),
            extra_packages: Vec::new(),
            aur_packages: Vec::new(),
            zram_swap: true,
            default_apps: true,
            provision: true,
        }
    }
}

impl InstallConfig {
    /// Load a configuration from a JSON file.
    pub fn from_json_file(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path).map_err(|e| Error::io(path, e))?;
        let config: Self = serde_json::from_str(&raw)?;
        Ok(config)
    }

    /// Serialize the configuration to pretty JSON, redacting nothing — callers
    /// that persist this must treat it as sensitive (it contains passwords).
    pub fn to_json(&self) -> Result<String> {
        Ok(serde_json::to_string_pretty(self)?)
    }

    /// All packages installed by `pacstrap`: the base set, the curated app set
    /// (when enabled), the zram tooling, plus user extras — de-duplicated and
    /// order-preserving.
    pub fn all_packages(&self) -> Vec<String> {
        let mut packages: Vec<String> = stack::BASE_PACKAGES
            .iter()
            .map(|p| (*p).to_owned())
            .collect();
        let push_unique = |packages: &mut Vec<String>, pkg: &str| {
            if !packages.iter().any(|p| p == pkg) {
                packages.push(pkg.to_owned());
            }
        };
        if self.default_apps {
            for app in stack::DEFAULT_APPS {
                push_unique(&mut packages, app);
            }
        }
        if self.zram_swap {
            push_unique(&mut packages, "zram-generator");
        }
        for extra in &self.extra_packages {
            push_unique(&mut packages, extra);
        }
        packages
    }

    /// Validate the configuration before any destructive action. Returns the
    /// first problem found, described for a human.
    pub fn validate(&self) -> Result<()> {
        if self.disk.trim().is_empty() {
            return Err(Error::Config("no target disk selected".into()));
        }
        if !Path::new(&self.disk).is_absolute() {
            return Err(Error::Config(format!(
                "disk path must be absolute, got `{}`",
                self.disk
            )));
        }
        validate_hostname(&self.hostname)?;
        validate_username(&self.user.username)?;
        if self.user.password.is_empty() {
            return Err(Error::Config(format!(
                "user `{}` has no password",
                self.user.username
            )));
        }
        if self.locale.trim().is_empty() {
            return Err(Error::Config("locale must not be empty".into()));
        }
        if self.timezone.trim().is_empty() {
            return Err(Error::Config("timezone must not be empty".into()));
        }
        for package in self.extra_packages.iter().chain(&self.aur_packages) {
            validate_package_name(package)?;
        }
        if !self.github_user.is_empty() {
            validate_github_user(&self.github_user)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_invalid_without_disk_and_password() {
        let config = InstallConfig::default();
        assert!(config.validate().is_err());
    }

    fn config_with(disk: &str, password: &str) -> InstallConfig {
        InstallConfig {
            disk: disk.to_owned(),
            user: UserAccount {
                username: "arch".to_owned(),
                password: Secret::new(password),
            },
            ..InstallConfig::default()
        }
    }

    #[test]
    fn a_complete_config_validates() {
        assert!(config_with("/dev/vda", "hunter2").validate().is_ok());
    }

    #[test]
    fn relative_disk_path_is_rejected() {
        assert!(config_with("vda", "hunter2").validate().is_err());
    }

    #[test]
    fn base_packages_include_the_configured_kernel() {
        // Guards against KERNEL and BASE_PACKAGES diverging into a bootloader
        // entry that points at a kernel image that was never installed.
        assert!(
            stack::BASE_PACKAGES.contains(&stack::KERNEL),
            "BASE_PACKAGES must install stack::KERNEL ({})",
            stack::KERNEL
        );
    }

    #[test]
    fn invalid_extra_package_is_rejected() {
        let mut config = config_with("/dev/vda", "pw");
        config.extra_packages = vec!["htop".into(), "rm -rf /".into()];
        assert!(config.validate().is_err());
    }

    #[test]
    fn all_packages_dedups_extras() {
        let config = InstallConfig {
            extra_packages: vec!["git".into(), "htop".into()],
            ..InstallConfig::default()
        };
        let packages = config.all_packages();
        assert_eq!(packages.iter().filter(|p| *p == "git").count(), 1);
        assert!(packages.iter().any(|p| p == "htop"));
    }

    #[test]
    fn config_roundtrips_through_json() {
        let config = config_with("/dev/vda", "pw");
        let json = config.to_json().unwrap();
        let parsed: InstallConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.disk, "/dev/vda");
        assert_eq!(parsed.user.password.expose(), "pw");
    }
}
