//! Step 5 — timezone, clock, locale, console keymap and hostname.

use super::{Context, Step};
use crate::error::Result;
use crate::system::{Command, probe, target_path};

/// Configures all locale-related settings in the target system.
pub struct Localization;

impl Step for Localization {
    fn name(&self) -> &'static str {
        "Configure localization"
    }

    fn run(&self, ctx: &mut Context<'_>) -> Result<()> {
        let config = ctx.config;

        // Timezone + hardware clock.
        ctx.info(format!("setting timezone to {}", config.timezone));
        ctx.sys.run(
            &Command::new("ln")
                .arg("-sf")
                .arg(format!("/usr/share/zoneinfo/{}", config.timezone))
                .arg("/etc/localtime")
                .in_chroot(),
        )?;
        ctx.sys
            .run(&Command::new("hwclock").arg("--systohc").in_chroot())?;

        // Locale: enable the chosen line in locale.gen, generate, and set LANG.
        ctx.info(format!("generating locale {}", config.locale));
        ctx.sys.write(
            &target_path("/etc/locale.gen"),
            &format!("{} UTF-8\n", config.locale),
        )?;
        ctx.sys.run(&Command::new("locale-gen").in_chroot())?;
        ctx.sys.write(
            &target_path("/etc/locale.conf"),
            &format!("LANG={}\n", config.locale),
        )?;

        // Console keymap.
        ctx.sys.write(
            &target_path("/etc/vconsole.conf"),
            &format!("KEYMAP={}\n", config.keymap),
        )?;

        // Wireless regulatory domain, derived from the timezone's country, so
        // Wi-Fi uses the right channels/power. Skipped when unknown.
        if let Some(country) = probe::country_from_timezone(&config.timezone) {
            ctx.info(format!("setting wireless regulatory domain to {country}"));
            ctx.sys.write(
                &target_path("/etc/conf.d/wireless-regdom"),
                &format!("WIRELESS_REGDOM=\"{country}\"\n"),
            )?;
        }

        // Hostname and the matching /etc/hosts entries.
        ctx.info(format!("setting hostname to {}", config.hostname));
        ctx.sys.write(
            &target_path("/etc/hostname"),
            &format!("{}\n", config.hostname),
        )?;
        ctx.sys
            .write(&target_path("/etc/hosts"), &hosts_file(&config.hostname))?;
        Ok(())
    }
}

/// Standard `/etc/hosts` content wiring loopback to the hostname.
fn hosts_file(hostname: &str) -> String {
    format!(
        "127.0.0.1\tlocalhost\n::1\t\tlocalhost\n127.0.1.1\t{hostname}.localdomain\t{hostname}\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_file_wires_hostname() {
        let hosts = hosts_file("arch");
        assert!(hosts.contains("127.0.0.1\tlocalhost"));
        assert!(hosts.contains("127.0.1.1\tarch.localdomain\tarch"));
    }

    #[test]
    fn no_regdom_written_for_a_region_only_timezone() {
        use crate::steps::test_support::{config, dry_actions};
        // config() uses timezone "UTC", which has no country in zone.tab.
        let actions = dry_actions(&Localization, &config());
        assert!(!actions.iter().any(|a| a.contains("wireless-regdom")));
    }
}
