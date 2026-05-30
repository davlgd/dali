#!/usr/bin/env python3
"""End-to-end test for DALI: a real, bootable Arch install inside QEMU/KVM.

Pipeline:
  1. Extract the kernel + initramfs from the Arch ISO and read its label.
  2. Build a small ext4 "payload" image holding the DALI binary and config.
  3. Boot the live ISO under UEFI (OVMF) via direct kernel boot, with the
     serial console wired up, and drive it: mount the payload, run DALI headless
     against the blank virtual disk, then power off.
  4. (Harness-only) append `console=ttyS0` to the installed boot entry so the
     installed system is observable over serial.
  5. Reboot from the now-installed disk and assert it reaches a login prompt.

Everything runs unprivileged: /dev/kvm is world-accessible, files are extracted
with bsdtar and the payload is built with `mkfs.ext4 -d` — no mounts, no sudo.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import select
import shutil
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path

OVMF_CODE = "/usr/share/edk2/x64/OVMF_CODE.4m.fd"
OVMF_VARS = "/usr/share/edk2/x64/OVMF_VARS.4m.fd"
ISO_KERNEL = "arch/boot/x86_64/vmlinuz-linux"
ISO_INITRD = "arch/boot/x86_64/initramfs-linux.img"


_ANSI_PATTERNS = [
    re.compile(r"\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)"),  # OSC (BEL- or ST-terminated)
    re.compile(r"\x1bP[^\x1b]*\x1b\\"),                # DCS
    re.compile(r"\x1b\[[0-9;?]*[ -/]*[@-~]"),          # CSI
    re.compile(r"\x1b[()][AB0]"),                       # charset selection
    re.compile(r"\x1b."),                               # any other escape
]


def strip_ansi(text: str) -> str:
    """Remove terminal control sequences so we can match on plain text.

    The serial console interleaves charset shifts and OSC titles into prompts
    (e.g. `root\x1b(B@archiso`), which would otherwise break naive matching.
    """
    for pat in _ANSI_PATTERNS:
        text = pat.sub("", text)
    return text.replace("\x0f", "").replace("\x0e", "")


def log(msg: str) -> None:
    print(f"\n\033[1;36m[e2e]\033[0m {msg}", flush=True)


def run(cmd: list[str], **kw) -> subprocess.CompletedProcess:
    print(f"    $ {' '.join(cmd)}", flush=True)
    return subprocess.run(cmd, check=True, **kw)


def require_tools() -> None:
    needed = ("qemu-system-x86_64", "qemu-img", "bsdtar", "mkfs.ext4", "blkid")
    missing = [t for t in needed if not shutil.which(t)]
    if missing:
        sys.exit(f"missing required tools: {', '.join(missing)}")
    for fw in (OVMF_CODE, OVMF_VARS):
        if not Path(fw).exists():
            sys.exit(f"missing OVMF firmware: {fw} (install edk2-ovmf)")


@dataclass
class Prepared:
    iso: Path
    kernel: Path
    initrd: Path
    label: str
    payload: Path
    disk: Path
    vars: Path


def prepare(work: Path, iso: Path, dali_bin: Path, config: Path) -> Prepared:
    work.mkdir(parents=True, exist_ok=True)

    log("extracting kernel + initramfs from the ISO")
    run(["bsdtar", "-xf", str(iso), "-C", str(work), ISO_KERNEL, ISO_INITRD])

    log("reading ISO label")
    label = subprocess.run(
        ["blkid", "-o", "value", "-s", "LABEL", str(iso)],
        check=True, capture_output=True, text=True,
    ).stdout.strip()
    print(f"    label = {label}")

    log("building payload image (dali binary + config)")
    payload_dir = work / "payload"
    payload_dir.mkdir(exist_ok=True)
    shutil.copy(dali_bin, payload_dir / "dali")
    os.chmod(payload_dir / "dali", 0o755)
    shutil.copy(config, payload_dir / "config.json")
    payload = work / "payload.img"
    run(["mkfs.ext4", "-q", "-F", "-d", str(payload_dir), str(payload), "64M"])

    log("creating blank 16 GiB target disk")
    disk = work / "disk.qcow2"
    run(["qemu-img", "create", "-f", "qcow2", str(disk), "16G"], stdout=subprocess.DEVNULL)

    vars_copy = work / "OVMF_VARS.fd"
    shutil.copy(OVMF_VARS, vars_copy)

    return Prepared(iso, work / ISO_KERNEL, work / ISO_INITRD, label, payload, disk, vars_copy)


class Vm:
    """A QEMU process whose serial console is wired to our stdio pipes."""

    def __init__(self, args: list[str], transcript: Path):
        full = ["qemu-system-x86_64", *args, "-display", "none", "-serial", "stdio"]
        print(f"    $ {' '.join(full)}", flush=True)
        self.proc = subprocess.Popen(
            full, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT, bufsize=0,
        )
        self.transcript = transcript.open("wb")
        self.raw = ""  # raw text; ANSI-stripped only when matching

    def expect(self, pattern: str, timeout: float) -> str:
        rx = re.compile(pattern)
        deadline = time.time() + timeout
        fd = self.proc.stdout.fileno()
        while time.time() < deadline:
            # Strip ANSI from the WHOLE buffer each time: escape sequences can be
            # split across read() boundaries, so per-chunk stripping corrupts them.
            cleaned = strip_ansi(self.raw)
            if rx.search(cleaned):
                return cleaned
            if self.proc.poll() is not None and not select.select([fd], [], [], 0)[0]:
                raise EOFError(f"QEMU exited before matching /{pattern}/")
            r, _, _ = select.select([fd], [], [], 1.0)
            if r:
                data = os.read(fd, 4096)
                if not data:
                    continue
                self.transcript.write(data)
                self.transcript.flush()
                text = data.decode("utf-8", "replace")
                sys.stdout.write(text)
                sys.stdout.flush()
                self.raw += text
                # Bound memory on long-running phases (e.g. pacstrap output).
                if len(self.raw) > 1_000_000:
                    self.raw = self.raw[-200_000:]
        raise TimeoutError(f"timed out after {timeout}s waiting for /{pattern}/")

    def send(self, line: str) -> None:
        self.proc.stdin.write((line + "\n").encode())
        self.proc.stdin.flush()

    def wait(self, timeout: float) -> int:
        try:
            return self.proc.wait(timeout)
        except subprocess.TimeoutExpired:
            self.proc.kill()
            return self.proc.wait()

    def kill(self) -> None:
        if self.proc.poll() is None:
            self.proc.kill()
        self.transcript.close()


def common_machine() -> list[str]:
    return [
        "-machine", "q35,accel=kvm", "-cpu", "host", "-m", "3072", "-smp", "2",
        "-nic", "user", "-no-reboot",
    ]


def firmware(p: Prepared) -> list[str]:
    return [
        "-drive", f"if=pflash,format=raw,unit=0,readonly=on,file={OVMF_CODE}",
        "-drive", f"if=pflash,format=raw,unit=1,file={p.vars}",
    ]


def phase_install(p: Prepared, work: Path) -> None:
    log("PHASE 1 — booting live ISO and running DALI")
    append = (
        f"archisobasedir=arch archisolabel={p.label} cow_spacesize=2G "
        "copytoram=n console=ttyS0,115200 systemd.unit=multi-user.target"
    )
    # Disks: vda = target, vdb = payload; plus the ISO as a cdrom for the squashfs.
    args = [
        *common_machine(), *firmware(p),
        "-kernel", str(p.kernel), "-initrd", str(p.initrd), "-append", append,
        "-drive", f"file={p.disk},if=virtio,format=qcow2",
        "-drive", f"file={p.payload},if=virtio,format=raw",
        "-drive", f"file={p.iso},media=cdrom",
    ]
    vm = Vm(args, work / "phase1.log")
    try:
        # archiso shows a serial login prompt (no autologin); root has no password.
        vm.expect(r"archiso login:", timeout=300)
        vm.send("root")
        vm.expect(r"root@archiso", timeout=60)
        vm.send("mkdir -p /payload && mount /dev/vdb /payload && echo MOUNTED_$?")
        vm.expect(r"MOUNTED_0", timeout=30)
        vm.send("/payload/dali --config /payload/config.json --yes; echo DALI_RC=$?")
        out = vm.expect(r"DALI_RC=\d", timeout=1800)
        rc = re.search(r"DALI_RC=(\d)", out)
        if not rc or rc.group(1) != "0":
            raise RuntimeError(f"DALI install failed (DALI_RC={rc.group(1) if rc else '?'})")
        # Surface the generated boot entry for diagnostics (UUID must be set).
        vm.send("echo ENTRY_START; cat /mnt/boot/loader/entries/arch.conf; echo ENTRY_END")
        vm.expect(r"ENTRY_END", timeout=30)
        # Make the installed system observable over serial for phase 2.
        vm.send(
            "sed -i 's#^options .*#& console=ttyS0,115200#' "
            "/mnt/boot/loader/entries/arch.conf && echo PATCHED_$?"
        )
        vm.expect(r"PATCHED_0", timeout=30)
        vm.send("poweroff")
        vm.wait(timeout=120)
    finally:
        vm.kill()
    log("PHASE 1 complete — DALI reported a successful install")


def phase_boot(p: Prepared, hostname: str, work: Path) -> None:
    log("PHASE 2 — rebooting from the installed disk")
    args = [
        *common_machine(), *firmware(p),
        "-drive", f"file={p.disk},if=virtio,format=qcow2",
    ]
    vm = Vm(args, work / "phase2.log")
    try:
        vm.expect(rf"{re.escape(hostname)} login:", timeout=180)
    finally:
        vm.kill()
    log("PHASE 2 complete — installed system booted to a login prompt")


def main() -> int:
    ap = argparse.ArgumentParser(description="DALI end-to-end test in QEMU/KVM")
    ap.add_argument("--iso", required=True, type=Path)
    ap.add_argument("--dali", required=True, type=Path, help="path to the dali binary")
    ap.add_argument("--config", required=True, type=Path)
    ap.add_argument("--work", default=Path("e2e-work"), type=Path)
    ap.add_argument(
        "--hostname",
        default=None,
        help="override the login hostname to wait for (defaults to the config's hostname, or 'arch')",
    )
    args = ap.parse_args()

    require_tools()
    # Phase 2 waits for "<hostname> login:"; derive it from the config so it
    # cannot drift out of sync (the config default hostname is 'arch').
    hostname = args.hostname or json.loads(args.config.read_text()).get("hostname", "arch")
    prepared = prepare(args.work, args.iso, args.dali, args.config)
    phase_install(prepared, args.work)
    phase_boot(prepared, hostname, args.work)
    log("\033[1;32mE2E PASSED\033[0m — real install booted successfully")
    return 0


if __name__ == "__main__":
    sys.exit(main())
