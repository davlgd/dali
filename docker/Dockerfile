# Reproducible build/lint/test environment for DALI.
#
# Uses the same base as the install target (Arch Linux). Arch's `rust` package
# bundles cargo, rustfmt and clippy, so `cargo clippy`/`fmt`/`test` behave
# exactly as on the live ISO.
FROM archlinux:latest

# `--noconfirm` keeps the build non-interactive; clean the cache to stay small.
RUN pacman -Syu --noconfirm rust && pacman -Scc --noconfirm

WORKDIR /dali

# Copy the manifests first so the dependency layer is cached until they change.
# Cargo.lock is included so the container resolves the exact same versions.
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY tests ./tests
COPY examples ./examples

# The default command runs the full quality gate.
CMD ["sh", "-c", "cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test"]
