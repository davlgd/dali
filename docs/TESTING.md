# Testing DALI

DALI is tested at three levels, from fast to thorough.

## 1. Unit tests

In-module `#[cfg(test)]` tests cover config validation, the package set, the
`Secret` redaction, command/chroot construction, partition-path derivation,
disk probing, the bootloader entry, and the TUI helpers.

```sh
cargo test
```

## 2. Integration test

`tests/dry_run.rs` runs the **built binary** in `--dry-run` mode against a
config file and asserts the emitted plan covers the whole pipeline (partition →
pacstrap → bootloader → users → services) and never claims a real install. This
is the cheapest way to validate the end-to-end plan and runs as part of
`cargo test`.

## 3. End-to-end: a real, bootable install in QEMU/KVM

`scripts/e2e.py` performs an actual Arch installation inside a throwaway VM and
verifies it boots.

### Prerequisites

- `qemu` (`qemu-base` is enough)
- `edk2-ovmf` — UEFI firmware (OVMF), so the live environment and the installed
  system both run in UEFI mode
- `bsdtar` (libarchive) — extracts the kernel/initramfs from the ISO
- `e2fsprogs` — `mkfs.ext4 -d` builds the payload image without mounting
- read/write access to `/dev/kvm` (no root needed if `/dev/kvm` is accessible)
- the Arch ISO:
  `curl -fLO https://geo.mirror.pkgbuild.com/iso/latest/archlinux-x86_64.iso`

### Run

```sh
cargo build --release
python3 scripts/e2e.py \
  --iso archlinux-x86_64.iso \
  --dali target/release/dali \
  --config examples/full.json \
  --hostname dali-test
```

Expect it to take several minutes (the VM downloads packages via `pacstrap`).

### How it works

1. **Prepare** — extract `vmlinuz-linux` + `initramfs-linux.img` and the ISO
   label with `bsdtar`/`blkid`; build a small ext4 *payload* image holding the
   `dali` binary and config; create a blank 16 GiB qcow2 target disk; copy the
   OVMF vars so NVRAM (the boot entry) persists across phases.
2. **Phase 1 (install)** — boot the live ISO under OVMF via direct kernel boot
   with `console=ttyS0`, drive the serial console (log in as root, mount the
   payload, run `dali --config … --yes`), then patch the installed boot entry
   to add `console=ttyS0` (a harness-only tweak so the installed system is
   observable) and power off.
3. **Phase 2 (boot)** — boot the now-installed disk under OVMF (no ISO) and
   assert it reaches a `<hostname> login:` prompt — proving systemd-boot, the
   kernel, the initramfs and the root subvolume all work together.

Logs for each phase are written to the `--work` directory (`phase1.log`,
`phase2.log`).

### Notes

- The harness assumes an **Arch host**: the OVMF firmware paths
  (`/usr/share/edk2/x64/OVMF_{CODE,VARS}.4m.fd`) and the ISO kernel/initramfs
  paths are hardcoded for Arch's layout. On another distro, adjust the
  constants at the top of `scripts/e2e.py`.
- Everything runs unprivileged: no host mounts, no loop devices, no `sudo`.
- The harness only modifies its own VM disk images; the host is never touched.
- The default release binary (glibc) matches the live ISO; no musl build is
  required for the test.
