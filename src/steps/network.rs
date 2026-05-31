//! Step — carry the live ISO's network profiles into the target, so a system
//! installed over Wi-Fi (or a configured wired connection) comes back online
//! after the first reboot without re-entering credentials.
//!
//! NetworkManager profiles are copied as-is. iwd profiles (the Arch ISO's
//! default Wi-Fi backend) are **converted** into NetworkManager keyfiles, so
//! the carried credentials actually work under NetworkManager's default
//! `wpa_supplicant` backend. Profiles hold secrets, so everything lands `0600`
//! in a `0700` directory (NetworkManager refuses world-readable profiles).
//! Best-effort: with nothing configured on the live system it is a clean no-op.

use std::path::Path;

use super::{Context, Step};
use crate::error::Result;
use crate::system::{Command, probe, target_path};

/// Carries NetworkManager / iwd profiles from the live system into the target.
pub struct CarryNetwork;

impl Step for CarryNetwork {
    fn name(&self) -> &'static str {
        "Carry network configuration"
    }

    fn run(&self, ctx: &mut Context<'_>) -> Result<()> {
        // Everything ends up as NetworkManager keyfiles in system-connections.
        let mut profiles = collect_nm(ctx);
        for (name, contents) in collect_iwd(ctx) {
            if let Some(converted) = iwd_to_nm(&name, &contents) {
                profiles.push(converted);
            }
        }
        if profiles.is_empty() {
            return Ok(());
        }

        let dir = probe::NM_CONNECTIONS_DIR;
        let target_dir = target_path(dir);
        ctx.info(format!("installing {} network profile(s)", profiles.len()));
        ctx.sys.mkdir_p(&target_dir)?;
        // Lock the directory to 0700 *before* writing secrets into it, so the
        // keyfiles are never reachable by other users (paths are chroot-relative).
        ctx.sys
            .run(&Command::new("chmod").arg("700").arg(dir).in_chroot())?;
        for (name, contents) in &profiles {
            ctx.sys.write(&format!("{target_dir}/{name}"), contents)?;
            ctx.sys.run(
                &Command::new("chmod")
                    .arg("600")
                    .arg(format!("{dir}/{name}"))
                    .in_chroot(),
            )?;
        }
        Ok(())
    }
}

/// Existing NetworkManager keyfiles, copied verbatim.
fn collect_nm(ctx: &Context<'_>) -> Vec<(String, String)> {
    read_profiles(
        ctx,
        probe::NM_CONNECTIONS_DIR,
        "live.nmconnection",
        String::new,
    )
}

/// Raw iwd profiles (filename + contents) to be converted.
fn collect_iwd(ctx: &Context<'_>) -> Vec<(String, String)> {
    read_profiles(ctx, probe::IWD_DIR, "Example.psk", || {
        "[Security]\nPassphrase=changeme\n".to_owned()
    })
}

/// `(filename, contents)` for each file in the live `dir`. On a dry-run the live
/// filesystem cannot be enumerated, so one representative entry is returned to
/// make the planned actions visible.
fn read_profiles(
    ctx: &Context<'_>,
    dir: &str,
    sample_name: &str,
    sample_contents: impl Fn() -> String,
) -> Vec<(String, String)> {
    if ctx.sys.is_real() {
        probe::list_files_in(Path::new(dir))
            .into_iter()
            .filter_map(|path| {
                let name = path.file_name()?.to_string_lossy().into_owned();
                let contents = std::fs::read_to_string(&path).ok()?;
                Some((name, contents))
            })
            .collect()
    } else {
        vec![(sample_name.to_owned(), sample_contents())]
    }
}

/// Convert an iwd profile to a NetworkManager keyfile `(filename, contents)`.
/// Handles PSK and open networks; enterprise (`.8021x`) and unknown types are
/// skipped (`None`).
fn iwd_to_nm(filename: &str, contents: &str) -> Option<(String, String)> {
    let (stem, kind) = filename.rsplit_once('.')?;
    let ssid = decode_iwd_ssid(stem);

    // The decoded SSID is attacker-advertised and becomes both an on-disk
    // filename and keyfile values. Reject anything that could escape the
    // directory (`/`) or break the keyfile grammar / inject lines (control
    // characters such as newline); iwd stores those bytes escaped, so a normal
    // SSID is unaffected.
    if ssid.is_empty() || ssid.contains('/') || ssid.chars().any(char::is_control) {
        return None;
    }

    let security = match kind {
        "psk" => {
            let psk = iwd_value(contents, "Passphrase")
                .or_else(|| iwd_value(contents, "PreSharedKey"))?;
            format!("\n[wifi-security]\nkey-mgmt=wpa-psk\npsk={psk}\n")
        }
        "open" => String::new(),
        _ => return None,
    };

    let nm = format!(
        "[connection]\nid={ssid}\ntype=wifi\n\n\
         [wifi]\nmode=infrastructure\nssid={ssid}\n{security}\n\
         [ipv4]\nmethod=auto\n\n[ipv6]\nmethod=auto\n"
    );
    Some((format!("{ssid}.nmconnection"), nm))
}

/// The value of a `key = value` line anywhere in an iwd profile (whitespace
/// around `=` tolerated).
fn iwd_value(contents: &str, key: &str) -> Option<String> {
    contents.lines().find_map(|line| {
        let (k, value) = line.split_once('=')?;
        (k.trim() == key).then(|| value.trim().to_owned())
    })
}

/// Decode iwd's filename encoding for an SSID. iwd stores an SSID verbatim when
/// it contains only `[A-Za-z0-9_ -]`; otherwise the *whole* SSID byte string is
/// hex-encoded as a single leading `=` followed by one contiguous lowercase-hex
/// run. A leading `=` is an unambiguous discriminator (it is never in the
/// verbatim set). Malformed input is passed through rather than turned to garbage.
fn decode_iwd_ssid(stem: &str) -> String {
    let Some(hex) = stem.strip_prefix('=') else {
        return stem.to_owned();
    };
    if hex.len() % 2 != 0 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return stem.to_owned();
    }
    let bytes: Vec<u8> = (0..hex.len())
        .step_by(2)
        .filter_map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::steps::test_support::{config, dry_actions};

    #[test]
    fn installs_profiles_into_network_manager() {
        let actions = dry_actions(&CarryNetwork, &config());
        let nm = probe::NM_CONNECTIONS_DIR;
        assert!(
            actions
                .iter()
                .any(|a| a.contains(&format!("mkdir -p /mnt{nm}")))
        );
        // The carried NM profile and the converted iwd profile both land here.
        assert!(
            actions
                .iter()
                .any(|a| a.contains(&format!("/mnt{nm}/live.nmconnection")))
        );
        assert!(
            actions
                .iter()
                .any(|a| a.contains(&format!("/mnt{nm}/Example.nmconnection")))
        );
        assert!(
            actions
                .iter()
                .any(|a| a.contains(&format!("chmod 700 {nm}")))
        );
        assert!(
            actions
                .iter()
                .any(|a| a.contains("chmod 600") && a.contains(".nmconnection"))
        );
    }

    #[test]
    fn converts_an_iwd_psk_profile_to_a_keyfile() {
        let (name, body) =
            iwd_to_nm("MyWifi.psk", "[Security]\nPassphrase=hunter2\n").expect("psk converts");
        assert_eq!(name, "MyWifi.nmconnection");
        assert!(body.contains("type=wifi"));
        assert!(body.contains("ssid=MyWifi"));
        assert!(body.contains("key-mgmt=wpa-psk"));
        assert!(body.contains("psk=hunter2"));
    }

    #[test]
    fn skips_enterprise_iwd_profiles() {
        assert!(iwd_to_nm("Corp.8021x", "[Security]\nEAP-Method=PEAP\n").is_none());
    }

    #[test]
    fn decodes_iwd_ssid_names() {
        // Safe-set SSIDs (incl. spaces) are stored verbatim; anything else is
        // the whole SSID hex-encoded behind a single leading `=`.
        assert_eq!(decode_iwd_ssid("PlainSSID"), "PlainSSID");
        assert_eq!(decode_iwd_ssid("My Wifi"), "My Wifi");
        assert_eq!(decode_iwd_ssid("=436166c3a9"), "Café");
        assert_eq!(decode_iwd_ssid("=612162"), "a!b");
        // Malformed (not hex / odd length / multibyte) passes through, no panic.
        assert_eq!(decode_iwd_ssid("=€x"), "=€x");
    }

    #[test]
    fn rejects_path_and_control_ssids() {
        // iwd stores `/` as `=2f` and a newline as `=0a`; both must be refused
        // rather than steer the write path or inject keyfile lines.
        assert!(iwd_to_nm("=2f.open", "").is_none());
        assert!(iwd_to_nm("=0a.psk", "[Security]\nPassphrase=x\n").is_none());
    }
}
