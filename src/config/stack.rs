//! The fixed, opinionated technical choices that define a DALI install.
//!
//! Keeping these as constants — rather than yet more knobs — is a deliberate
//! KISS decision. This is the inventory file: what gets installed and enabled.

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
/// Base packages every install receives — the bootable minimum (sorted).
pub const BASE_PACKAGES: &[&str] = &[
    "base",
    "base-devel",
    "btrfs-progs",
    "curl",
    "git",
    "linux",
    "linux-firmware",
    "networkmanager",
    "snap-pac",
    "snapper",
    "sudo",
    "vim",
];
/// Curated application set installed by default (official repos), on top of
/// [`BASE_PACKAGES`]. Toggled by `InstallConfig::default_apps`. Sorted.
pub const DEFAULT_APPS: &[&str] = &[
    "atuin",
    "avahi",
    "bash-completion",
    "bat",
    "docker",
    "docker-buildx",
    "ffmpeg",
    "glab",
    "impala",
    "jless",
    "jq",
    "lazydocker",
    "lazygit",
    "less",
    "minio-client",
    "nano",
    "openssh",
    "uv",
    "whois",
    "yt-dlp",
    "zellij",
];
/// Base services enabled in every install (sorted). `systemd-boot-update`
/// keeps the ESP copy of systemd-boot current across upgrades; `fstrim.timer`
/// runs periodic TRIM (SSD/NVMe).
pub const SERVICES: &[&str] = &[
    "NetworkManager",
    "fstrim.timer",
    "systemd-boot-update.service",
    "systemd-timesyncd",
];
/// Services enabled only when the default app set is installed (their units
/// ship with `avahi` / `docker` / `openssh`). Sorted.
pub const APP_SERVICES: &[&str] = &["avahi-daemon.service", "docker.service", "sshd.service"];
/// Tools installed globally during provisioning via `mise use -g`. Sorted.
pub const MISE_GLOBAL_TOOLS: &[&str] = &["bun", "codex", "gemini", "node", "opencode", "pi"];
