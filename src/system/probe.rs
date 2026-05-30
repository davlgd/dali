//! Read-only inspection of the live environment.
//!
//! Probing is intentionally dependency-free and side-effect-free: it reads
//! `/sys`, `/proc` and the network, never mutating anything. That is why it
//! lives outside the [`Sys`](super::Sys) effects boundary and runs identically
//! in real and dry-run modes.

use std::fmt;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::Path;
use std::time::Duration;

/// A block device the user can install onto.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Disk {
    /// Device path, e.g. `/dev/vda`.
    pub path: String,
    /// Capacity in bytes.
    pub size_bytes: u64,
    /// Human-readable model string, if the kernel exposes one.
    pub model: Option<String>,
}

impl Disk {
    /// Capacity formatted in binary units (GiB), for display.
    #[allow(clippy::cast_precision_loss)] // display only; disk sizes are far below 2^52 bytes
    pub fn size_human(&self) -> String {
        const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
        format!("{:.1} GiB", self.size_bytes as f64 / GIB)
    }
}

impl fmt::Display for Disk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.model {
            Some(model) => write!(f, "{} ({}, {})", self.path, self.size_human(), model),
            None => write!(f, "{} ({})", self.path, self.size_human()),
        }
    }
}

/// Whether the machine booted in UEFI mode (required for systemd-boot).
///
/// `/sys/firmware/efi` is the canonical signal; `efivars` is a separate
/// efivarfs mount that is not always present, so checking it would yield false
/// negatives.
pub fn is_uefi() -> bool {
    Path::new("/sys/firmware/efi").is_dir()
}

/// The CPU microcode package matching this machine's vendor, if recognised.
///
/// Microcode is host-specific (it depends on the CPU being installed onto), so
/// it is probed at install time rather than stored in the config.
pub fn cpu_microcode() -> Option<&'static str> {
    let cpuinfo = std::fs::read_to_string("/proc/cpuinfo").ok()?;
    let vendor = cpuinfo
        .lines()
        .find_map(|line| line.strip_prefix("vendor_id"))?;
    if vendor.contains("GenuineIntel") {
        Some("intel-ucode")
    } else if vendor.contains("AuthenticAMD") {
        Some("amd-ucode")
    } else {
        None
    }
}

/// Whether the current process runs as root (effective UID 0).
///
/// Read from `/proc/self/status` to avoid a libc dependency. Returns `false`
/// if the file cannot be read (e.g. non-Linux), which is the safe default.
pub fn is_root() -> bool {
    let Ok(status) = std::fs::read_to_string("/proc/self/status") else {
        return false;
    };
    status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .and_then(|uids| uids.split_whitespace().nth(1))
        .is_some_and(|euid| euid == "0")
}

/// Whether we can reach the network, tested by opening a short-lived TCP
/// connection to the Arch package mirror redirector on port 443.
pub fn has_network() -> bool {
    can_connect("archlinux.org:443", Duration::from_secs(5))
}

fn can_connect(host: &str, timeout: Duration) -> bool {
    let Ok(mut addrs) = host.to_socket_addrs() else {
        return false;
    };
    addrs.any(|addr| TcpStream::connect_timeout(&addr, timeout).is_ok())
}

/// Enumerate fixed block devices suitable for installation, sorted by path.
///
/// Reads `/sys/block`, skipping virtual and removable-by-nature devices
/// (loopback, ram disks, optical, device-mapper, zram).
pub fn list_disks() -> Vec<Disk> {
    let mut disks = Vec::new();
    let Ok(entries) = std::fs::read_dir("/sys/block") else {
        return disks;
    };

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_virtual_device(&name) {
            continue;
        }
        let base = entry.path();
        // Only real backing devices expose a `device` directory.
        if !base.join("device").exists() {
            continue;
        }
        let Some(size_bytes) = read_size_bytes(&base) else {
            continue;
        };
        if size_bytes == 0 {
            continue;
        }
        disks.push(Disk {
            path: format!("/dev/{name}"),
            size_bytes,
            model: read_model(&base),
        });
    }

    disks.sort_by(|a, b| a.path.cmp(&b.path));
    disks
}

fn is_virtual_device(name: &str) -> bool {
    const VIRTUAL_PREFIXES: &[&str] = &["loop", "ram", "sr", "dm-", "zram", "md", "fd"];
    VIRTUAL_PREFIXES.iter().any(|p| name.starts_with(p))
}

fn read_size_bytes(base: &Path) -> Option<u64> {
    let sectors: u64 = std::fs::read_to_string(base.join("size"))
        .ok()?
        .trim()
        .parse()
        .ok()?;
    // The kernel always reports `size` in 512-byte sectors regardless of the
    // device's physical sector size.
    Some(sectors * 512)
}

fn read_model(base: &Path) -> Option<String> {
    let model = std::fs::read_to_string(base.join("device/model")).ok()?;
    let trimmed = model.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

/// Available UTF-8 glibc locales, e.g. `en_US.UTF-8`, sorted. Empty off Arch.
pub fn list_locales() -> Vec<String> {
    std::fs::read_to_string("/usr/share/i18n/SUPPORTED")
        .map(|s| parse_locales(&s))
        .unwrap_or_default()
}

/// Parse glibc's `SUPPORTED` file, keeping the UTF-8 locale identifiers.
fn parse_locales(supported: &str) -> Vec<String> {
    let mut locales: Vec<String> = supported
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let name = parts.next()?;
            // Second column is the charset; we only support UTF-8 installs.
            (parts.next() == Some("UTF-8")).then(|| name.to_owned())
        })
        .collect();
    locales.sort();
    locales.dedup();
    locales
}

/// Available console keymaps, e.g. `fr`, `us`, `uk`, sorted. Empty off Arch.
pub fn list_keymaps() -> Vec<String> {
    let mut keymaps = Vec::new();
    collect_keymaps(Path::new("/usr/share/kbd/keymaps"), &mut keymaps);
    keymaps.sort();
    keymaps.dedup();
    keymaps
}

fn collect_keymaps(dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_keymaps(&path, out);
        } else if let Some(name) = keymap_name(&entry.file_name().to_string_lossy()) {
            out.push(name.to_owned());
        }
    }
}

/// The keymap name from a file like `fr.map.gz` → `fr` (None if not a keymap).
fn keymap_name(filename: &str) -> Option<&str> {
    filename
        .strip_suffix(".map.gz")
        .or_else(|| filename.strip_suffix(".map"))
}

/// Available timezones in `Region/City` form, e.g. `Europe/Paris`, sorted.
/// Empty off Arch.
pub fn list_timezones() -> Vec<String> {
    // zone1970.tab is the canonical, clean list (one TZ per relevant line);
    // fall back to the older zone.tab.
    let table = std::fs::read_to_string("/usr/share/zoneinfo/zone1970.tab")
        .or_else(|_| std::fs::read_to_string("/usr/share/zoneinfo/zone.tab"))
        .unwrap_or_default();
    parse_timezones(&table)
}

/// Parse a `zone.tab`/`zone1970.tab` file: the timezone is the 3rd tab field.
fn parse_timezones(table: &str) -> Vec<String> {
    let mut zones: Vec<String> = table
        .lines()
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| line.split('\t').nth(2))
        .map(ToOwned::to_owned)
        .collect();
    zones.push("UTC".to_owned());
    zones.sort();
    zones.dedup();
    zones
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_locales_keeps_only_utf8_identifiers() {
        let supported = "en_US.UTF-8 UTF-8\nen_US ISO-8859-1\nfr_FR.UTF-8 UTF-8\n# comment\n";
        assert_eq!(parse_locales(supported), ["en_US.UTF-8", "fr_FR.UTF-8"]);
    }

    #[test]
    fn keymap_name_strips_extensions() {
        assert_eq!(keymap_name("fr.map.gz"), Some("fr"));
        assert_eq!(keymap_name("us.map"), Some("us"));
        assert_eq!(keymap_name("README"), None);
    }

    #[test]
    fn parse_timezones_takes_third_field_and_adds_utc() {
        let table = "#code\tcoordinates\tTZ\tcomments\n\
                     FR\t+4852+00220\tEurope/Paris\n\
                     JP\t+353916+1394441\tAsia/Tokyo\n";
        let zones = parse_timezones(table);
        assert!(zones.contains(&"Europe/Paris".to_owned()));
        assert!(zones.contains(&"Asia/Tokyo".to_owned()));
        assert!(zones.contains(&"UTC".to_owned()));
        // sorted
        assert!(zones.windows(2).all(|w| w[0] <= w[1]));
    }

    #[test]
    fn size_human_formats_gib() {
        let disk = Disk {
            path: "/dev/vda".into(),
            size_bytes: 20 * 1024 * 1024 * 1024,
            model: None,
        };
        assert_eq!(disk.size_human(), "20.0 GiB");
    }

    #[test]
    fn disk_display_includes_model_when_present() {
        let disk = Disk {
            path: "/dev/sda".into(),
            size_bytes: 1024 * 1024 * 1024,
            model: Some("Samsung SSD".into()),
        };
        assert!(disk.to_string().contains("Samsung SSD"));
        assert!(disk.to_string().contains("/dev/sda"));
    }

    #[test]
    fn virtual_devices_are_filtered() {
        assert!(is_virtual_device("loop0"));
        assert!(is_virtual_device("zram0"));
        assert!(is_virtual_device("sr0"));
        assert!(!is_virtual_device("vda"));
        assert!(!is_virtual_device("nvme0n1"));
        assert!(!is_virtual_device("sda"));
    }
}
