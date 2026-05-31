# DALI — Davlgd Arch Linux Installer

DALI is an **opinionated, single-binary installer for Arch Linux**. Where
[`archinstall`](https://github.com/archlinux/archinstall) exposes every option,
DALI commits to one specific configuration: download one static binary, run it
from the live ISO, answer a handful of questions, reboot into a working system.

> ⚠️ DALI **erases the target disk**. It is meant to be run from the official
> Arch Linux live ISO on a machine you intend to install onto.

## The opinionated stack

DALI installs exactly one well-trodden configuration, so there is nothing to
get wrong:

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

The only things you choose are: the **disk**, **hostname**, **username +
password**, optional **root password**, **locale**, **keymap**, **timezone**,
the zram toggle and any **extra packages**. Everything else is fixed by DALI's
opinionated stack.

More precisely, every install gets:

- **Base packages**: `base`, `base-devel`, `btrfs-progs`, `curl`, `git`,
  `linux`, `linux-firmware`, `networkmanager`, `snap-pac`, `snapper`, `sudo`,
  `vim` — plus the matching CPU microcode (`intel-ucode`/`amd-ucode`),
  `zram-generator` when zram is on, and any extras you list.
- **Default app set** (`default_apps`, on by default): `atuin`, `avahi`,
  `bash-completion`, `bat`, `docker`, `docker-buildx`, `ffmpeg`, `glab`,
  `impala`, `jless`, `jq`, `lazydocker`, `lazygit`, `less`, `minio-client`,
  `nano`, `openssh`, `uv`, `whois`, `yt-dlp`, `zellij` — with `docker.service`,
  `avahi-daemon.service` and `sshd.service` enabled and the user added to the
  `docker` group.
- **SSH keys** (optional, `github_user`): when set, that GitHub account's public
  keys (`https://github.com/<user>.keys`) are imported as the user's accepted
  SSH keys (`~/.ssh/authorized_keys`).
- **Snapshots**: `snapper` is configured for the root subvolume only (on the
  `@snapshots` subvol), and `snap-pac` takes automatic pre/post snapshots around
  every `pacman` transaction — so a bad upgrade can be rolled back. `/home` is
  deliberately not snapshotted.
- **Shell environment**: the user's `~/.bashrc` gets `~/.local/bin` on `PATH`,
  `mise` activation, helper functions (`check`, `clean_cargo`, `f`, `mkcd`, `up`,
  `w` — `up` updates the system + AUR, mise tools, global bun packages, uv
  tools and the V compiler in one go)
  and aliases (`add`/`list`/`remove`/`search` for pacman, `gac`/`gl`/`gst`/`gsw`/…,
  `dps`, `myip`, `pgen`).
- **Provisioning** (`provision`, on by default, best-effort): bootstraps the
  [`paru`](https://github.com/Morganamilo/paru) AUR helper, installs any
  `aur_packages` you list through it, builds the [V compiler](https://vlang.io)
  from source into `~/.local/bin`, runs the [`mise`](https://mise.jdx.dev) and
  [Claude Code](https://claude.com/claude-code) installers as your user, and
  installs `bun`, `codex`, `gemini`, `node`, `opencode` and `pi` globally via
  `mise`. By default it also installs the `kernel-modules-hook` AUR package
  (keeps the running kernel's modules across an upgrade) and enables
  `linux-modules-cleanup.service`. Network-bound; failures are reported as
  warnings and never abort the (already bootable) install. (`pamac-aur` is
  intentionally not in the defaults — it currently needs an older `libalpm`
  than Arch ships; add it to `aur_packages` once it is compatible again.)
- **System tuning** (always applied): `fs.inotify.max_user_watches = 524288`
  (a dev box exhausts the default almost immediately) and a systemd
  `DefaultLimitNOFILE=65536:524288` bump (system + user managers), and
  `net.ipv4.tcp_mtu_probing = 1` (robust SSH/transfers behind broken PMTUD).
- **ESP**: FAT32, 1 GiB, mounted at `/boot`.
- **Base services**: `NetworkManager`, `systemd-timesyncd`,
  `systemd-boot-update`, `fstrim.timer`.
- **Omitted-field defaults**: `hostname=arch`, `timezone=UTC`,
  `locale=en_US.UTF-8`, `keymap=us`, `zram_swap=true`, `default_apps=true`,
  `provision=true`, and root **locked** (empty `root_password`).

Set `"default_apps": false` for a bare bootable system, or `"provision": false`
to skip the AUR/`mise`/Claude Code step. When a real install finishes, DALI
**reboots into the new system by default** (immediately with `--yes`, after a
confirmation otherwise); pass `--no-reboot` to stay on the live environment.

## Install

Each GitHub release ships a static x86-64 binary. From the live ISO:

```sh
curl -fLO https://github.com/davlgd/dali/releases/latest/download/dali-linux-x86_64-musl
chmod +x dali-linux-x86_64-musl
./dali-linux-x86_64-musl
```

Verify it against the published `SHA256SUMS` if you like. A glibc build
(`dali-linux-x86_64-gnu`) is also attached for environments that prefer it.

## Requirements

DALI runs from an Arch-based live environment **booted in UEFI mode**, as
**root** — the [Arch Linux live ISO](https://archlinux.org/download/) or
[SystemRescue](https://www.system-rescue.org/Download/). A real install refuses
to proceed otherwise. The interactive TUI needs a genuine
terminal; on any other Linux box you can still rehearse the whole plan with
`cargo run -- --dry-run --config examples/minimal.toml`, which changes nothing.

## Usage

From the live ISO:

```sh
# Interactive (a single-screen TUI with sensible defaults pre-filled):
./dali

# See exactly what would happen, changing nothing:
./dali --dry-run --config myconfig.toml

# Fully automated, no prompts (for scripted / repeatable installs):
./dali --config myconfig.toml --yes
```

### Flags

| Flag                 | Effect                                                       |
|----------------------|--------------------------------------------------------------|
| `--config <FILE>`    | Install non-interactively from a TOML config (see below).    |
| `--dry-run`          | Print the exact plan of actions and exit without changes.    |
| `--yes`              | Skip the final "erase the disk" confirmation. Requires `--config`. |
| `--save-config <F>`  | Write the effective config (from `--config`, or from the wizard if none) to a file and exit. Conflicts with `--dry-run`/`--yes`. |
| `--no-reboot`        | Do not reboot at the end (default is to reboot into the new system). |

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
extra_packages = ["htop"]
zram_swap = true

# The [user] table must come last (TOML forbids bare keys after a table).
[user]
username = "david"
password = "changeme"
```

An empty `root_password` **locks the root account**; administration then
happens exclusively through the sudo-enabled user.

A config file may contain plaintext passwords — treat it as a secret.
`--save-config foo.toml` keeps them out of the shareable file: it writes
`foo.toml` **without** passwords and a sibling `foo.credentials.toml` (mode
`0600`) holding only the secrets. Passing `--config foo.toml` later merges the
sidecar back in automatically; a single file with inline passwords also works.

The interactive TUI gathers every one of these fields, including the zram
toggle and extra packages, and re-asks each password for confirmation. Locale,
keymap and timezone are **picked from a filterable list** of what the system
actually supports (press Enter on the field, type to filter, arrow-select) — no
need to remember exact identifiers.

## Building

DALI targets the **Rust 2024 edition** (minimum supported Rust version:
**1.85**).

```sh
cargo build --release        # produces target/release/dali
```

The default build is **dynamically linked against glibc** — fine on the Arch
live ISO, which ships the same glibc. For a portable, fully static binary
(recommended for shipping to arbitrary environments):

```sh
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl
```

"Fully static" refers only to the `dali` executable's own linkage: at runtime
DALI still drives the Arch live ISO toolchain (`sgdisk`, `mkfs.btrfs`,
`pacstrap`, `bootctl`, `genfstab`, `blkid`, …) as subprocesses, so it is meant
to run from that environment regardless of how it was linked.

## Development

```sh
cargo test                                  # unit + integration tests
cargo clippy --all-targets -- -D warnings   # zero-warning policy (pedantic)
cargo fmt --check                           # formatting
./scripts/ci.sh                             # all of the above in a clean Arch container

# Rehearse the whole install plan on any Linux box — no Arch, root or hardware
# needed, changes nothing:
cargo run -- --dry-run --config examples/minimal.toml
```

DALI is tested at three levels — unit, a process-level dry-run integration
test, and a real bootable QEMU/KVM install. See
[`docs/TESTING.md`](docs/TESTING.md).

### Architecture

The crate is split into small, single-responsibility modules:

- `config` — the opinionated install spec (host-specific bits like CPU
  microcode are probed at install time, not stored here).
- `system` — the **effects boundary** (`Sys` trait): run a command, write a
  file, or merely record a dry-run plan. This one seam is what makes DALI both
  safe to rehearse and easy to test. Read-only inspection lives in
  `system::probe`.
- `steps` — the ordered install pipeline; each step does exactly one thing and
  depends only on a `Context`.
- `tui` — the interactive wizard that produces a `config`.
- `cli` — the clap command-line argument definitions.
- `report` — user-facing progress output.
- `error` — the shared `Error`/`Result` type used across the crate.
- `app` — wires the above together for the binary.

Adding or reordering a step is a one-line change to `steps::pipeline()`.

### End-to-end testing

`scripts/e2e.py` performs a **real, bootable installation** inside a QEMU/KVM
virtual machine: it boots the Arch ISO, runs DALI headless against a virtual
disk, then reboots from that disk and asserts the installed system comes up to
a login prompt.

```sh
cargo build --release
python3 scripts/e2e.py \
  --iso archlinux-x86_64.iso \
  --dali target/release/dali \
  --config examples/full.toml
```

Requires `qemu`, `edk2-ovmf`, `bsdtar`, `e2fsprogs` and access to `/dev/kvm`.
See [`docs/TESTING.md`](docs/TESTING.md) for the full prerequisites, how the
harness works, and how to get the ISO.

## License

Apache-2.0 — see [LICENSE](LICENSE). Copyright 2026 davlgd.
