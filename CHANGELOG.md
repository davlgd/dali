# Changelog

All notable changes to DALI are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.0] - 2026-06-01

### Added

- Wi-Fi carry-over now actually works under NetworkManager: iwd profiles (the
  live ISO's default Wi-Fi backend) are **converted** into NetworkManager
  keyfiles instead of copied inert, so a system installed over Wi-Fi comes back
  online after the first reboot. Secrets land `0600` in a `0700` directory.
- Security hardening for the default app set: an sshd drop-in
  (`PermitRootLogin no`, `MaxAuthTries 3`) and a `ufw` firewall (deny incoming,
  allow outgoing, keep SSH reachable) applied by a first-boot one-shot service.
- `ufw` is now part of the default app set.
- `--completions <shell>` generates shell completions and `--man` generates a
  man page, both written to standard output.

### Changed

- When a step fails, the install transcript now records the failing step's
  reason instead of only marking the run as failed.

### Fixed

- The target's `/etc/resolv.conf` is removed before being rewritten, so a
  packaged symlink is replaced by a real file rather than being followed.

## [0.2.1] - 2026-06-01

### Changed

- The console login banner now shows the machine's **LAN IPv4** (the egress
  address, never the `docker0` bridge) via a NetworkManager dispatcher, and the
  redundant message of the day was removed.

### Fixed

- `atuin` is now functional in Bash: `bash-preexec` is installed and `atuin` is
  wired into `~/.bashrc`.

## [0.2.0] - 2026-05-31

### Added

- **TOML configuration** with an optional sidecar `*.credentials.toml`, so the
  shareable config can be kept free of plaintext passwords.
- **Btrfs root snapshots**: `snapper` is configured for the root subvolume only
  (on the `@snapshots` subvolume) and `snap-pac` takes pre/post snapshots around
  every `pacman` transaction.
- **System tuning**: `fs.inotify.max_user_watches`, a systemd
  `DefaultLimitNOFILE` bump, and `net.ipv4.tcp_mtu_probing`.
- **Network carry-over**: the live environment's NetworkManager/iwd profiles are
  copied into the installed system (mode `0600`).
- **Wireless regulatory domain** derived from the configured timezone.
- **pacman tuning** (`ParallelDownloads`, `Color`, `VerbosePkgLists`) on the live
  system and the target, mirror ranking via `reflector`, and reliable GnuPG
  keyservers in `dirmngr.conf`.
- **Provenance**: `/etc/dali-release` plus additive `DALI_*` fields in
  `/etc/os-release` (it stays `ID=arch`).
- **Login banners**: `/etc/issue` shows the live IPv4 at the console prompt, and
  `/etc/motd` is a short factual welcome.
- **Install diagnostics**: the transcript is saved to `/var/log/dali-install.log`
  and a per-step completion map to `/var/log/dali-steps.toml`.
- **`custom_commands`**: optional shell commands run as the user at the end of
  provisioning.
- An **`up`** command and a **`list`** alias in the shell environment.
- `fastfetch` and `htop` added to the default app set.
- Kernel cmdline pins `rootfstype=btrfs` and disables `zswap` when zram is on.

### Changed

- Configuration is now **TOML** instead of JSON (`examples/*.toml`).
- The `~/.bashrc` block is marker-delimited and rewritten idempotently, with a
  one-time `~/.bashrc.dali.bak` backup.
- Internal reorganization (`config/` module split) and substantially expanded
  test coverage.

### Fixed

- Restore user ownership of `~/.bashrc` after it is written as root.
- Make revocation of the temporary provisioning sudo grant tamper-evident, so a
  passwordless-sudo drop-in can never silently persist into the installed system.

## [0.1.2] - 2026-05-31

### Added

- `mise` activation in `~/.bashrc` and global developer tools installed via
  `mise` (`node`, `bun`, `codex`, `gemini`, `opencode`, `pi`).
- pacman `add` / `remove` / `search` shell aliases.

### Changed

- No default username — it must be chosen explicitly.
- Code and configuration lists are kept in alphabetical order.

## [0.1.1] - 2026-05-31

### Added

- `less` and `whois` added to the default app set.
- `~/.local/bin` is placed on `PATH` system-wide via `/etc/profile.d`.
- A `pgen` password-generator shell alias.

### Changed

- The installer reboots into the new system by default; pass `--no-reboot` to
  stay on the live environment.

## [0.1.0] - 2026-05-31

- Initial release: opinionated minimal Arch Linux installer (UEFI + GPT,
  systemd-boot, Btrfs subvolumes, NetworkManager, optional zram) with an
  interactive TUI, a headless config mode, and a dry-run. Imports a GitHub
  account's SSH keys, installs `uv` and builds the V compiler, and seeds a set
  of shell aliases.
