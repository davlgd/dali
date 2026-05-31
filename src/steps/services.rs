//! Step — enable the services that make the installed system usable, and
//! configure zram swap when requested.

use super::{Context, Step};
use crate::config::stack;
use crate::error::Result;
use crate::system::{Command, target_path};

/// Enables base services (networking, time sync) and optional zram swap.
pub struct Services;

impl Step for Services {
    fn name(&self) -> &'static str {
        "Enable services"
    }

    fn run(&self, ctx: &mut Context<'_>) -> Result<()> {
        let app_services = if ctx.config.default_apps {
            stack::APP_SERVICES
        } else {
            &[]
        };
        for service in stack::SERVICES.iter().chain(app_services) {
            ctx.info(format!("enabling {service}"));
            ctx.sys.run(
                &Command::new("systemctl")
                    .arg("enable")
                    .arg(*service)
                    .in_chroot(),
            )?;
        }

        if ctx.config.zram_swap {
            ctx.info("configuring zram swap");
            ctx.sys
                .write(&target_path("/etc/systemd/zram-generator.conf"), ZRAM_CONF)?;
        }
        Ok(())
    }
}

/// zram-generator config: a zstd-compressed swap device sized to RAM, capped at 8 GiB.
const ZRAM_CONF: &str = "[zram0]\nzram-size = min(ram, 8192)\ncompression-algorithm = zstd\n";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::steps::test_support::{config, dry_actions};

    #[test]
    fn app_services_are_enabled_only_with_the_default_app_set() {
        let mut cfg = config();

        cfg.default_apps = true;
        let actions = dry_actions(&Services, &cfg);
        // Base services always; app services (docker/avahi/sshd) only here.
        assert!(actions.iter().any(|a| a.contains("enable NetworkManager")));
        assert!(actions.iter().any(|a| a.contains("enable docker.service")));

        cfg.default_apps = false;
        let actions = dry_actions(&Services, &cfg);
        assert!(actions.iter().any(|a| a.contains("enable NetworkManager")));
        assert!(
            !actions.iter().any(|a| a.contains("docker.service")),
            "app services must be skipped without the app set"
        );
    }

    #[test]
    fn zram_config_is_written_only_when_enabled() {
        let mut cfg = config();

        cfg.zram_swap = true;
        let actions = dry_actions(&Services, &cfg);
        assert!(actions.iter().any(|a| a.contains("zram-generator.conf")));

        cfg.zram_swap = false;
        let actions = dry_actions(&Services, &cfg);
        assert!(!actions.iter().any(|a| a.contains("zram-generator.conf")));
    }

    #[test]
    fn zram_config_caps_size_and_uses_zstd() {
        assert!(ZRAM_CONF.contains("zram-size = min(ram, 8192)"));
        assert!(ZRAM_CONF.contains("compression-algorithm = zstd"));
    }
}
