#!/usr/bin/env bash
# Run DALI's full quality gate inside a clean Arch Linux container, so results
# are reproducible regardless of the host. Falls back to a bare `cargo` run if
# Docker is unavailable.
#
# Usage: scripts/ci.sh
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

run_native() {
  echo "==> Running quality gate natively"
  cargo fmt --check
  cargo clippy --all-targets -- -D warnings
  cargo test
}

if command -v docker >/dev/null 2>&1; then
  echo "==> Building CI image (archlinux + rust)"
  docker build -t dali-ci -f docker/Dockerfile .
  echo "==> Running quality gate in container"
  docker run --rm dali-ci
else
  echo "warning: docker not found, falling back to native run" >&2
  run_native
fi
