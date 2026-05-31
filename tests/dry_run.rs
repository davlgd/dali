//! Integration test: drive the built binary end-to-end in `--dry-run` mode and
//! assert it produces a coherent, complete install plan without touching the
//! host. This exercises CLI parsing, config loading, and the whole step
//! pipeline through the real process boundary.

use std::process::Command;

/// Write a config to a temp file, run `dali --dry-run --config <file>`, and
/// return its captured stdout. `tag` keeps concurrent tests in separate dirs.
fn run_dry(tag: &str, config_toml: &str) -> (bool, String) {
    let dir = std::env::temp_dir().join(format!("dali-it-{}-{tag}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let config = dir.join("config.toml");
    std::fs::write(&config, config_toml).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_dali"))
        .arg("--dry-run")
        .arg("--config")
        .arg(&config)
        .output()
        .expect("failed to run dali binary");

    let _ = std::fs::remove_dir_all(&dir);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    (output.status.success(), stdout)
}

const VALID_CONFIG: &str = r#"
disk = "/dev/vda"
hostname = "dali-it"
extra_packages = ["htop"]

[user]
username = "tester"
password = "secret"
"#;

#[test]
fn dry_run_emits_a_complete_plan() {
    let (ok, out) = run_dry("valid", VALID_CONFIG);
    assert!(ok, "dry-run should exit successfully\n{out}");

    // The plan must cover the whole pipeline, in the user's-eye order.
    for needle in [
        "Partition disk",
        "sgdisk",
        "mkfs.btrfs",
        "pacstrap",
        "genfstab",
        "bootctl",
        "useradd",
        "systemctl enable NetworkManager",
        "/var/log/dali-install.log",
        "/var/log/dali-steps.toml",
        "Dry run complete",
    ] {
        assert!(out.contains(needle), "plan missing `{needle}`\n{out}");
    }

    // A dry-run must never claim a real install happened.
    assert!(!out.contains("Installation complete"));
}

#[test]
fn invalid_config_is_rejected_without_side_effects() {
    // Missing disk and password — validation must fail before any action.
    let (ok, out) = run_dry("invalid", "hostname = \"x\"\n");
    assert!(!ok, "invalid config should fail");
    assert!(
        !out.contains("sgdisk"),
        "no destructive action should be planned"
    );
}
