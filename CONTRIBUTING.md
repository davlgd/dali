# Contributing to DALI

Thanks for your interest! DALI is small and opinionated on purpose — the goal
is one well-trodden install path, not endless options. Keep changes in that
spirit.

## Quality gate

All of these must pass before a change lands. GitHub Actions runs this same
gate on every push and pull request (see `.github/workflows/ci.yml`);
`scripts/ci.sh` reproduces it locally in a clean Arch container:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings   # zero warnings, pedantic included
cargo test
```

`./scripts/ci.sh` runs the whole gate (in Docker if available, natively
otherwise).

## Architecture in one minute

```
config   →  what to install (the single source of truth)
system   →  the Sys effects boundary: run a command / write a file, OR record
            a dry-run plan. Read-only inspection lives in system::probe.
steps    →  the ordered install pipeline; each step does ONE thing.
tui      →  the interactive wizard that produces a config.
cli      →  the clap command-line argument definitions.
report   →  user-facing progress output.
error    →  the shared Error/Result type.
app      →  wires it together for the binary.
```

## Adding or changing an install step

A step is a unit struct implementing `steps::Step`:

```rust
pub struct MyStep;

impl Step for MyStep {
    fn name(&self) -> &'static str { "Do the thing" }
    fn run(&self, ctx: &mut Context<'_>) -> Result<()> {
        ctx.info("explaining what happens");
        ctx.sys.run(&Command::new("…").arg("…"))?;       // host command
        ctx.sys.run(&Command::new("…").in_chroot())?;    // inside the target
        ctx.sys.write(&target_path("/etc/thing"), "…")?; // write a file
        Ok(())
    }
}
```

Then add it to the ordered list in `steps::pipeline()` — that one line is the
only wiring needed. Never reach for `std::process`/`std::fs` directly in a
step: always go through `ctx.sys` so the action is dry-run-able and testable.

## Verifying a change

Rehearse the full plan without touching anything:

```sh
cargo run -- --dry-run --config examples/full.toml
```

For real-system changes, run the QEMU end-to-end test (see
[`docs/TESTING.md`](docs/TESTING.md)).

## Scope / out of scope

The current stack is a plain, unencrypted Btrfs root on UEFI + systemd-boot.
Disk encryption (LUKS), LVM/RAID, and multi-disk layouts are **out of scope**
for now — they would require `mkinitcpio.conf` `HOOKS` changes and a larger
config surface that conflicts with DALI's "one good path" premise.
