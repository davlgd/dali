# DALI — Davlgd Arch Linux Installer

DALI is an **opinionated, single-binary installer for Arch Linux**. Where
[`archinstall`](https://github.com/archlinux/archinstall) exposes every option,
DALI commits to one specific configuration: download one static binary, run it
from the live ISO, answer a handful of questions, reboot into a working system.

> ⚠️ DALI **erases the target disk**. It is meant to be run from the official
> Arch Linux live ISO on a machine you intend to install onto.

## The opinionated stack

DALI installs exactly one well-trodden configuration:

| Area        | Choice                                                        |
|-------------|---------------------------------------------------------------|
| Firmware    | UEFI + GPT                                                    |
| Bootloader  | systemd-boot                                                  |
| Filesystem  | Btrfs with `@`, `@home`, `@log`, `@pkg`, `@snapshots` subvols |
| Compression | `zstd`, `noatime`                                             |
| Kernel      | `linux`                                                       |
| Network     | NetworkManager                                                |
| Swap        | zram (zstd, sized to RAM, capped at 8 GiB) — optional         |
| Admin       | a sudo-enabled user in the `wheel` group; root locked by default |

The only things you choose are the **disk**, **hostname**, **username +
password**, optional **root password**, **locale**, **keymap**, **timezone**,
the zram toggle and any **extra packages**. Everything else is fixed by the
stack above — see [What every install sets up](#what-every-install-sets-up).

## Requirements

DALI runs from an Arch-based live environment **booted in UEFI mode**, as
**root** — the [Arch Linux live ISO](https://archlinux.org/download/) or
[SystemRescue](https://www.system-rescue.org/Download/). A real install refuses
to proceed otherwise. (On any other Linux box you can still rehearse the whole
plan with `--dry-run`, which changes nothing — see [Usage](#usage).)

## Install

Each GitHub release ships a static x86-64 binary. From the live ISO:

```sh
curl -fLO https://github.com/davlgd/dali/releases/latest/download/dali-linux-x86_64-musl
chmod +x dali-linux-x86_64-musl
./dali-linux-x86_64-musl
```

Verify it against the published `SHA256SUMS` if you like. A glibc build
(`dali-linux-x86_64-gnu`) is also attached for environments that prefer it.

## Usage

```sh
# Interactive: a single-screen TUI with sensible defaults pre-filled.
./dali

# See exactly what would happen, changing nothing.
./dali --dry-run --config myconfig.toml

# Fully automated, no prompts (scripted / repeatable installs).
./dali --config myconfig.toml --yes
```

When a real install finishes, DALI **reboots into the new system by default**
(immediately with `--yes`, after a confirmation otherwise); pass `--no-reboot`
to stay on the live environment. Set `default_apps = false` for a bare bootable
system, or `provision = false` to skip the post-install tooling step.

### Flags

| Flag                 | Effect                                                       |
|----------------------|--------------------------------------------------------------|
| `--config <FILE>`    | Install non-interactively from a TOML config (see below).    |
| `--dry-run`          | Print the exact plan of actions and exit without changes.    |
| `--yes`              | Skip the final "erase the disk" confirmation. Requires `--config`. |
| `--save-config <F>`  | Write the effective config (from `--config`, or from the wizard if none) to a file and exit. Conflicts with `--dry-run`/`--yes`. |
| `--no-reboot`        | Do not reboot at the end (default is to reboot into the new system). |

`--completions <shell>` prints a shell completion script and `--man` prints a
man page, both to stdout:

```sh
./dali --completions bash | sudo tee /usr/share/bash-completion/completions/dali
./dali --man | sudo tee /usr/share/man/man1/dali.1 > /dev/null
```

### Configuration file

The configuration is **TOML**. Every field except `disk` and `user` has a
sensible default and may be omitted. The smallest useful config
([`examples/minimal.toml`](examples/minimal.toml)):

```toml
disk = "/dev/vda"

[user]
username = "arch"
password = "changeme"
```

A fully specified config ([`examples/full.toml`](examples/full.toml)):

```toml
disk = "/dev/vda"
hostname = "dali-test"
timezone = "Europe/Paris"
locale = "en_US.UTF-8"
keymap = "fr"
root_password = ""
extra_packages = ["neovim"]
zram_swap = true

# The [user] table must come last (TOML forbids bare keys after a table).
[user]
username = "david"
password = "changeme"
```

- An empty `root_password` **locks the root account**; administration then
  happens exclusively through the sudo-enabled user.
- A config file may contain plaintext passwords — treat it as a secret.
  `--save-config foo.toml` keeps them out of the shareable file: it writes
  `foo.toml` **without** passwords and a sibling `foo.credentials.toml` (mode
  `0600`) holding only the secrets. `--config foo.toml` later merges the sidecar
  back in automatically; a single file with inline passwords also works.

The interactive TUI gathers every one of these fields and re-asks each password
for confirmation. Locale, keymap and timezone are **picked from a filterable
list** of what the system actually supports (press Enter on the field, type to
filter, arrow-select) — no need to remember exact identifiers.

## What every install sets up

### Disk, boot & packages

- **ESP**: FAT32, 1 GiB, mounted at `/boot`.
- **Base packages**: `base`, `base-devel`, `btrfs-progs`, `curl`, `git`,
  `linux`, `linux-firmware`, `networkmanager`, `snap-pac`, `snapper`, `sudo`,
  `vim` — plus the matching CPU microcode (`intel-ucode`/`amd-ucode`),
  `zram-generator` when zram is on, and any extras you list.
- **Default app set** (`default_apps`, on by default): `atuin`, `avahi`,
  `bash-completion`, `bash-preexec`, `bat`, `docker`, `docker-buildx`,
  `fastfetch`, `ffmpeg`, `glab`, `htop`, `impala`, `jless`, `jq`, `lazydocker`,
  `lazygit`, `less`, `minio-client`, `nano`, `openssh`, `ufw`, `uv`, `whois`,
  `yt-dlp`, `zellij` — with `docker.service`, `avahi-daemon.service` and
  `sshd.service` enabled and the user added to the `docker` group.
- **Base services**: `NetworkManager`, `systemd-timesyncd`,
  `systemd-boot-update`, `fstrim.timer`.

### Snapshots & recovery

- **`snapper`** is configured for the root subvolume only (on the `@snapshots`
  subvol), and **`snap-pac`** takes automatic pre/post snapshots around every
  `pacman` transaction — so a bad upgrade can be rolled back. `/home` is
  deliberately not snapshotted.

### Shell & developer tooling

- **Shell environment**: the user's `~/.bashrc` gets `~/.local/bin` on `PATH`,
  `mise` and `atuin` activation (the latter via `bash-preexec`), helper
  functions (`check`, `clean_cargo`, `f`, `mkcd`, `up`, `w`) and aliases
  (`add`/`list`/`remove`/`search` for pacman, `gac`/`gl`/`gst`/`gsw`/…, `dps`,
  `myip`, `pgen`). `up` updates the system, mise tools, global bun/uv packages
  and the V compiler in one go. The block is marker-delimited, so re-running
  replaces it in place (with a one-time `~/.bashrc.dali.bak` backup).
- **Provisioning** (`provision`, on by default, best-effort): builds the
  [V compiler](https://vlang.io) into `~/.local/bin`, runs the
  [`mise`](https://mise.jdx.dev) and [Claude Code](https://claude.com/claude-code)
  installers, and installs `bun`, `codex`, `gemini`, `node`, `opencode` and `pi`
  globally via `mise`. Any `custom_commands` you list run as your user at the end
  of this step. Network-bound; failures are warnings, never aborting the
  (already bootable) install.
- **SSH keys** (optional, `github_user`): that GitHub account's public keys
  (`https://github.com/<user>.keys`) are imported into `~/.ssh/authorized_keys`.

### Networking & mirrors

- **Mirrors**: `reflector` ranks the mirrorlist by speed, filtered to the
  timezone's country (worldwide fallback), before `pacstrap` — so both the
  install and the installed system pull from fast mirrors.
- **pacman tuning**: `Color`, `ParallelDownloads = 5` and `VerbosePkgLists`,
  applied to the live system (faster install) and the target.
- **Network carry-over**: the live environment's connections are carried into
  the target (mode `0600`) so a Wi-Fi install reconnects after reboot — NM
  profiles as-is, and iwd profiles (the ISO's Wi-Fi backend) converted to
  NetworkManager keyfiles so they work under NM's default backend.
- **Wireless regdom**: the regulatory domain is derived from the timezone's
  country (e.g. `Europe/Paris` → `FR`), so Wi-Fi uses the right channels/power.
- **GnuPG keyservers**: `/etc/gnupg/dirmngr.conf` gets several keyservers and a
  short timeout, so `pacman-key` doesn't hang on a dead server.

### System tuning, identity & diagnostics

- **System tuning**: `fs.inotify.max_user_watches = 524288`, a systemd
  `DefaultLimitNOFILE = 65536:524288` bump, and `net.ipv4.tcp_mtu_probing = 1`.
- **Hardening** (with the app set): an sshd drop-in disables root SSH and caps
  auth tries, and `ufw` is configured on first boot (deny incoming, allow
  outgoing, keep SSH reachable).
- **Login banner**: a NetworkManager dispatcher keeps `/etc/issue` showing the
  machine's LAN IPv4 at the console login prompt — so you can see where to SSH
  in (the egress address, never the `docker0` bridge).
- **Provenance**: `/etc/dali-release` records that DALI provisioned the system
  (and which version), and `/etc/os-release` gains additive `DALI_*` fields
  while keeping `ID=arch` / `PRETTY_NAME="Arch Linux"` — it stays Arch.
- **Install log**: the full transcript is written to `/var/log/dali-install.log`
  and a per-step completion map to `/var/log/dali-steps.toml`, for diagnosis.

### Defaults

Omitted config fields default to `hostname=arch`, `timezone=UTC`,
`locale=en_US.UTF-8`, `keymap=us`, `zram_swap=true`, `default_apps=true`,
`provision=true`, and a **locked** root (empty `root_password`).

## Building

DALI targets the **Rust 2024 edition** (minimum supported Rust: **1.85**).

```sh
cargo build --release                                    # dynamic glibc build
rustup target add x86_64-unknown-linux-musl              # for a portable, fully
cargo build --release --target x86_64-unknown-linux-musl # static binary
```

"Fully static" refers only to `dali`'s own linkage: at runtime it still drives
the Arch live ISO toolchain (`sgdisk`, `mkfs.btrfs`, `pacstrap`, `bootctl`,
`genfstab`, …) as subprocesses, so it is meant to run from that environment.

## Contributing & testing

The quality gate, architecture, and how to add an install step are in
[`CONTRIBUTING.md`](CONTRIBUTING.md). DALI is tested at three levels — unit, a
process-level dry-run integration test, and a real bootable QEMU/KVM install —
documented in [`docs/TESTING.md`](docs/TESTING.md).

The quickest check, on any Linux box (no Arch, root or hardware needed, changes
nothing):

```sh
cargo run -- --dry-run --config examples/minimal.toml
```

## License

Apache-2.0 — see [LICENSE](LICENSE). Copyright 2026 davlgd.
