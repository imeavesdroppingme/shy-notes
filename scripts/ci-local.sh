#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."

echo "==> interaction-core unit + e2e replay"
cargo test -p interaction-core

echo "==> clippy (interaction-core)"
cargo clippy -p interaction-core -- -D warnings

echo "==> frontend typecheck + build"
pnpm install --frozen-lockfile 2>/dev/null || pnpm install
pnpm exec tsc --noEmit
pnpm build

echo "==> tauri check"
cargo check -p shy-notes

echo "==> e2e binary smoke"
bash tests/e2e/smoke.sh

echo "CI local OK"
