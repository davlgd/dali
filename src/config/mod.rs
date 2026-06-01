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

use std::path::{Path, PathBuf};

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

/// Post-install provisioning options. Every sub-step is best-effort and
/// network-bound; disable any that aren't wanted. Serializes as `[provision]`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(from = "ProvisionRepr")]
pub struct ProvisionSettings {
    /// Master switch: when false the whole provisioning step is skipped.
    pub enabled: bool,
    /// Build the V compiler from source and symlink it into `~/.local/bin`.
    pub v: bool,
    /// Install `mise` (with its global tool set) and Claude Code — the AI/dev
    /// CLI tooling.
    pub tools: bool,
}

impl Default for ProvisionSettings {
    fn default() -> Self {
        // Route through `yes()` so the "default on" value has a single source,
        // shared with the per-field `#[serde(default = "yes")]` below.
        Self {
            enabled: yes(),
            v: yes(),
            tools: yes(),
        }
    }
}

/// Deserialization shim: accept either the modern `[provision]` table or a
/// legacy bare `provision = true|false` (pre-0.4), which maps to the master
/// switch alone. Keeps old config files loading with their intent intact.
#[derive(Deserialize)]
#[serde(untagged)]
enum ProvisionRepr {
    Legacy(bool),
    Detailed {
        #[serde(default = "yes")]
        enabled: bool,
        #[serde(default = "yes")]
        v: bool,
        #[serde(default = "yes")]
        tools: bool,
    },
}

impl From<ProvisionRepr> for ProvisionSettings {
    fn from(repr: ProvisionRepr) -> Self {
        match repr {
            ProvisionRepr::Legacy(enabled) => Self {
                enabled,
                ..Self::default()
            },
            ProvisionRepr::Detailed { enabled, v, tools } => Self { enabled, v, tools },
        }
    }
}

/// Shell environment options. Serializes as `[shell]`.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Shell {
    /// Write the DALI alias/function block into the user's `~/.bashrc`.
    pub aliases: bool,
}

impl Default for Shell {
    fn default() -> Self {
        Self { aliases: true }
    }
}

/// serde default helper: `true`.
fn yes() -> bool {
    true
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
    /// Root password. If empty, the root account is locked and administration
    /// happens exclusively through the sudo-enabled [`Self::user`].
    pub root_password: Secret,
    /// Optional GitHub username. When set, that account's public keys
    /// (`https://github.com/<user>.keys`) are imported as the user's accepted
    /// SSH keys. Empty means "don't import".
    pub github_user: String,
    /// Whether sshd accepts password authentication. `None` (the default) is
    /// resolved at install time: password auth is disabled when SSH keys are
    /// imported (a non-empty [`Self::github_user`]) and kept otherwise, so a
    /// keyless box is never locked out. `Some(_)` is always honored.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssh_password_auth: Option<bool>,
    /// Extra packages to install on top of [`stack::BASE_PACKAGES`].
    pub extra_packages: Vec<String>,
    /// Enable a compressed RAM swap device (zram) sized to total RAM, capped at
    /// 8 GiB.
    pub zram_swap: bool,
    /// Install the curated [`stack::DEFAULT_APPS`] set and enable their services
    /// ([`stack::APP_SERVICES`]: docker, avahi, sshd). Disable for a bare
    /// bootable system.
    pub default_apps: bool,
    /// Nameservers written to the target's `/etc/resolv.conf` for the duration of
    /// the chroot (NetworkManager takes over after first boot). Empty leaves
    /// whatever `pacstrap` copied in place.
    pub dns_servers: Vec<String>,
    /// Country to restrict the `reflector` mirror ranking to (a name or code,
    /// e.g. `France` or `FR`, comma-separated for several). Empty (the default)
    /// ranks mirrors worldwide by speed.
    pub mirror_country: String,
    /// Optional shell commands run as the user, inside the target, near the end
    /// of provisioning (best-effort). Requires [`ProvisionSettings::enabled`].
    pub custom_commands: Vec<String>,
    /// Post-install provisioning options. Serializes as the `[provision]` table,
    /// so it is kept among the trailing tables (TOML forbids bare keys after a
    /// table at the same level).
    pub provision: ProvisionSettings,
    /// Shell environment options. Serializes as the `[shell]` table.
    pub shell: Shell,
    /// The administrator account to create (member of `wheel`, sudo-enabled).
    ///
    /// Kept last so it serializes as a trailing `[user]` TOML table.
    pub user: UserAccount,
}

impl Default for InstallConfig {
    fn default() -> Self {
        Self {
            disk: String::new(),
            hostname: "arch".to_owned(),
            timezone: "UTC".to_owned(),
            locale: "en_US.UTF-8".to_owned(),
            keymap: "us".to_owned(),
            root_password: Secret::default(),
            github_user: String::new(),
            ssh_password_auth: None,
            extra_packages: Vec::new(),
            zram_swap: true,
            default_apps: true,
            dns_servers: vec!["9.9.9.9".to_owned(), "1.1.1.1".to_owned()],
            mirror_country: String::new(),
            custom_commands: Vec::new(),
            provision: ProvisionSettings::default(),
            shell: Shell::default(),
            user: UserAccount {
                // No default username on purpose — the user must choose one.
                username: String::new(),
                password: Secret::default(),
            },
        }
    }
}

impl InstallConfig {
    /// Load a configuration from a TOML file.
    ///
    /// If a sibling credentials file ([`credentials_path`]) exists, its
    /// passwords are merged in (only non-empty values override, so a locked
    /// root stays locked). A single file with inline secrets still works.
    pub fn from_toml_file(path: &Path) -> Result<Self> {
        let raw = std::fs::read_to_string(path).map_err(|e| Error::io(path, e))?;
        let mut config: Self = toml::from_str(&raw)?;

        let creds_path = credentials_path(path);
        if creds_path.exists() {
            let raw =
                std::fs::read_to_string(&creds_path).map_err(|e| Error::io(&creds_path, e))?;
            let creds: Credentials = toml::from_str(&raw)?;
            if !creds.user_password.is_empty() {
                config.user.password = creds.user_password;
            }
            if !creds.root_password.is_empty() {
                config.root_password = creds.root_password;
            }
        }
        Ok(config)
    }

    /// Serialize the configuration to pretty TOML, redacting nothing (it
    /// contains passwords). Test-only: real persistence goes through
    /// [`Self::to_toml_safe`] + [`Self::to_credentials_toml`].
    #[cfg(test)]
    pub(crate) fn to_toml(&self) -> Result<String> {
        Ok(toml::to_string_pretty(self)?)
    }

    /// Serialize the configuration to TOML with the passwords blanked, so the
    /// result is safe to share or commit.
    pub fn to_toml_safe(&self) -> Result<String> {
        let mut safe = self.clone();
        safe.user.password = Secret::default();
        safe.root_password = Secret::default();
        Ok(toml::to_string_pretty(&safe)?)
    }

    /// Serialize just the secrets (passwords) to TOML, for the sidecar
    /// credentials file.
    pub fn to_credentials_toml(&self) -> Result<String> {
        let creds = Credentials {
            user_password: self.user.password.clone(),
            root_password: self.root_password.clone(),
        };
        Ok(toml::to_string_pretty(&creds)?)
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
        for package in &self.extra_packages {
            validate_package_name(package)?;
        }
        for ns in &self.dns_servers {
            if ns.parse::<std::net::IpAddr>().is_err() {
                return Err(Error::Config(format!(
                    "invalid DNS server `{ns}` (expected an IP address)"
                )));
            }
        }
        let country = self.mirror_country.trim();
        if country.starts_with('-') || country.chars().any(char::is_control) {
            return Err(Error::Config(format!(
                "invalid mirror_country `{}`",
                self.mirror_country
            )));
        }
        if !self.github_user.is_empty() {
            validate_github_user(&self.github_user)?;
        }
        Ok(())
    }
}

/// The sidecar credentials file for a given config path: `foo.toml` →
/// `foo.credentials.toml`.
pub(crate) fn credentials_path(main: &Path) -> PathBuf {
    let stem = main
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("config");
    main.with_file_name(format!("{stem}.credentials.toml"))
}

/// The secret half of a saved configuration, kept out of the shareable file.
#[derive(Default, Serialize, Deserialize)]
struct Credentials {
    #[serde(default)]
    user_password: Secret,
    #[serde(default)]
    root_password: Secret,
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
    fn app_services_have_their_packages_in_default_apps() {
        // Guards against APP_SERVICES (enabled with the app set) drifting from
        // the package that ships each unit — which would fail to enable.
        let owners = [
            ("avahi-daemon.service", "avahi"),
            ("docker.service", "docker"),
            ("sshd.service", "openssh"),
        ];
        for service in stack::APP_SERVICES {
            let Some((_, package)) = owners.iter().find(|(s, _)| s == service) else {
                panic!("APP_SERVICES entry `{service}` has no known package");
            };
            assert!(
                stack::DEFAULT_APPS.contains(package),
                "`{package}` (owner of `{service}`) must be in DEFAULT_APPS"
            );
        }
    }

    #[test]
    fn invalid_dns_server_is_rejected() {
        let mut config = config_with("/dev/vda", "pw");
        config.dns_servers = vec!["9.9.9.9".into(), "not-an-ip".into()];
        assert!(config.validate().is_err());
    }

    #[test]
    fn empty_dns_servers_is_allowed() {
        let mut config = config_with("/dev/vda", "pw");
        config.dns_servers.clear();
        assert!(config.validate().is_ok());
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
    fn config_roundtrips_through_toml() {
        let config = config_with("/dev/vda", "pw");
        let toml = config.to_toml().unwrap();
        let parsed: InstallConfig = toml::from_str(&toml).unwrap();
        assert_eq!(parsed.disk, "/dev/vda");
        assert_eq!(parsed.user.password.expose(), "pw");
    }

    #[test]
    fn to_toml_serializes_and_reparses() {
        // Guards the field-ordering trap: `[user]` must be the trailing table,
        // otherwise TOML serialization fails (bare key after a table).
        let toml = config_with("/dev/vda", "pw").to_toml().unwrap();
        assert!(toml.contains("[user]"), "expected a [user] table:\n{toml}");
        assert!(toml::from_str::<InstallConfig>(&toml).is_ok());
    }

    #[test]
    fn credentials_path_appends_credentials_suffix() {
        assert_eq!(
            credentials_path(Path::new("/etc/foo.toml")),
            Path::new("/etc/foo.credentials.toml")
        );
    }

    #[test]
    fn safe_toml_omits_secrets_and_credentials_toml_keeps_them() {
        let config = config_with("/dev/vda", "hunter2");
        let safe = config.to_toml_safe().unwrap();
        assert!(
            !safe.contains("hunter2"),
            "safe config must not leak password"
        );
        let creds = config.to_credentials_toml().unwrap();
        assert!(creds.contains("hunter2"));
        assert!(creds.contains("user_password"));
    }

    #[test]
    fn split_save_then_load_round_trips_the_password() {
        let dir = std::env::temp_dir().join(format!("dali-creds-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let main = dir.join("c.toml");
        let config = config_with("/dev/vda", "hunter2");
        std::fs::write(&main, config.to_toml_safe().unwrap()).unwrap();
        std::fs::write(
            credentials_path(&main),
            config.to_credentials_toml().unwrap(),
        )
        .unwrap();

        let loaded = InstallConfig::from_toml_file(&main).unwrap();
        assert_eq!(loaded.user.password.expose(), "hunter2");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn provision_and_shell_tables_round_trip() {
        let mut config = config_with("/dev/vda", "pw");
        config.provision.v = false;
        config.shell.aliases = false;
        let toml = config.to_toml().unwrap();
        assert!(toml.contains("[provision]"));
        assert!(toml.contains("[shell]"));
        let parsed: InstallConfig = toml::from_str(&toml).unwrap();
        assert!(parsed.provision.enabled);
        assert!(!parsed.provision.v);
        assert!(parsed.provision.tools);
        assert!(!parsed.shell.aliases);
    }

    #[test]
    fn legacy_bare_provision_bool_still_loads() {
        // Pre-0.4 configs used `provision = true|false` at the top level; it must
        // keep mapping to the master switch (sub-toggles default on).
        let off: InstallConfig = toml::from_str(
            "disk = \"/dev/vda\"\nprovision = false\n[user]\nusername=\"a\"\npassword=\"p\"\n",
        )
        .unwrap();
        assert!(!off.provision.enabled);
        assert!(off.provision.v && off.provision.tools);

        let on: InstallConfig = toml::from_str(
            "disk = \"/dev/vda\"\nprovision = true\n[user]\nusername=\"a\"\npassword=\"p\"\n",
        )
        .unwrap();
        assert!(on.provision.enabled && on.provision.v && on.provision.tools);
    }

    #[test]
    fn omitted_provision_and_shell_default_to_on() {
        let cfg: InstallConfig =
            toml::from_str("disk = \"/dev/vda\"\n[user]\nusername=\"a\"\npassword=\"p\"\n")
                .unwrap();
        assert!(cfg.provision.enabled && cfg.provision.v && cfg.provision.tools);
        assert!(cfg.shell.aliases);
    }

    #[test]
    fn single_file_with_inline_secrets_still_loads() {
        let dir = std::env::temp_dir().join(format!("dali-inline-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let main = dir.join("c.toml");
        std::fs::write(
            &main,
            config_with("/dev/vda", "inlinepw").to_toml().unwrap(),
        )
        .unwrap();

        let loaded = InstallConfig::from_toml_file(&main).unwrap();
        assert_eq!(loaded.user.password.expose(), "inlinepw");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
