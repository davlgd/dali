//! Step 9 — enable the services that make the installed system usable, and
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
        for service in stack::SERVICES {
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
