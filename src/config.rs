//! Installation configuration: the single source of truth describing *what*
//! DALI will install.
//!
//! DALI is **opinionated minimal**: the technical stack is fixed (UEFI + GPT,
//! Btrfs root, systemd-boot, the `linux` kernel, NetworkManager). The only
//! things the user actually decides are captured in [`InstallConfig`]. Keeping
//! the fixed choices as constants — rather than yet more knobs — is a
//! deliberate KISS decision.

use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

/// The fixed, opinionated technical choices that define a DALI install.
pub mod stack {
    /// Root filesystem. Btrfs gives us subvolumes and snapshots for free.
    pub const FILESYSTEM: &str = "btrfs";
    /// EFI System Partition filesystem.
    pub const ESP_FILESYSTEM: &str = "fat32";
    /// Size of the EFI System Partition.
    pub const ESP_SIZE_MIB: u64 = 1024;
    /// The kernel package installed by default.
    pub const KERNEL: &str = "linux";
    /// Mountpoint of the EFI System Partition inside the installed system.
    pub const ESP_MOUNT: &str = "/boot";
    /// Where the target root is mounted on the live system during install.
    pub const TARGET_MOUNT: &str = "/mnt";
    /// Btrfs subvolume layout: (subvolume name, relative mountpoint).
    pub const SUBVOLUMES: &[(&str, &str)] = &[
        ("@", "/"),
        ("@home", "/home"),
        ("@log", "/var/log"),
        ("@pkg", "/var/cache/pacman/pkg"),
        ("@snapshots", "/.snapshots"),
    ];
    /// Base packages every install receives.
    pub const BASE_PACKAGES: &[&str] = &[
        "base",
        "linux",
        "linux-firmware",
        "btrfs-progs",
        "networkmanager",
        "sudo",
        "vim",
        "git",
        "base-devel",
    ];
    /// Services enabled in the installed system. `systemd-boot-update` keeps
    /// the ESP copy of systemd-boot current across future systemd upgrades;
    /// `fstrim.timer` runs periodic TRIM, recommended for SSD/NVMe.
    pub const SERVICES: &[&str] = &[
        "NetworkManager",
        "systemd-timesyncd",
        "systemd-boot-update.service",
        "fstrim.timer",
    ];
}

/// A secret string (e.g. a password) that never reveals itself in `Debug`
/// output or logs.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    /// Wrap a plaintext secret.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Borrow the underlying plaintext. Use sparingly and never log the result.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// Whether the secret is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_empty() {
            f.write_str("Secret(<empty>)")
        } else {
            f.write_str("Secret(<redacted>)")
        }
    }
}

impl From<&str> for Secret {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

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
    /// Extra packages to install on top of [`stack::BASE_PACKAGES`].
    pub extra_packages: Vec<String>,
    /// Enable a compressed RAM swap device (zram) sized to available memory.
    pub zram_swap: bool,
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
                username: "arch".to_owned(),
                password: Secret::default(),
            },
            root_password: Secret::default(),
            extra_packages: Vec::new(),
            zram_swap: true,
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

    /// All packages to install: the fixed base set, the zram tooling when
    /// enabled, plus user extras — all de-duplicated and order-preserving.
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
        Ok(())
    }
}

/// Hostnames: 1–63 chars, alphanumeric or hyphen, not starting/ending with a hyphen.
fn validate_hostname(name: &str) -> Result<()> {
    let valid = (1..=63).contains(&name.len())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        && !name.starts_with('-')
        && !name.ends_with('-');
    if valid {
        Ok(())
    } else {
        Err(Error::Config(format!("invalid hostname `{name}`")))
    }
}

/// Linux usernames: start with a lowercase letter or underscore, followed by
/// lowercase letters, digits, underscores or hyphens; at most 32 chars.
fn validate_username(name: &str) -> Result<()> {
    let mut chars = name.chars();
    let head_ok = matches!(chars.next(), Some(c) if c.is_ascii_lowercase() || c == '_');
    let tail_ok =
        chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-');
    if head_ok && tail_ok && (1..=32).contains(&name.len()) {
        Ok(())
    } else {
        Err(Error::Config(format!("invalid username `{name}`")))
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
    fn secret_never_leaks_in_debug() {
        let secret = Secret::new("topsecret");
        assert_eq!(format!("{secret:?}"), "Secret(<redacted>)");
        assert!(!format!("{secret:?}").contains("topsecret"));
    }

    #[test]
    fn hostname_rules() {
        assert!(validate_hostname("arch").is_ok());
        assert!(validate_hostname("my-arch-01").is_ok());
        assert!(validate_hostname("-bad").is_err());
        assert!(validate_hostname("bad-").is_err());
        assert!(validate_hostname("").is_err());
        assert!(validate_hostname("white space").is_err());
    }

    #[test]
    fn username_rules() {
        assert!(validate_username("arch").is_ok());
        assert!(validate_username("_svc").is_ok());
        assert!(validate_username("1bad").is_err());
        assert!(validate_username("Bad").is_err());
        assert!(validate_username("").is_err());
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
