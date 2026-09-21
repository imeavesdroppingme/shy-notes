#!/usr/bin/env bash
# Smoke-launch the built shy-notes binary, verify it stays alive briefly, then stop it.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"

BIN=""
for candidate in \
  "$ROOT/target/release/shy-notes" \
  "$CARGO_TARGET_DIR/release/shy-notes" \
  "$ROOT/src-tauri/target/release/shy-notes"
do
  if [[ -x "$candidate" ]]; then
    BIN="$candidate"
    break
  fi
done

# Also accept macOS app bundle executable.
APP_EXEC="$ROOT/target/release/bundle/macos/shy-notes.app/Contents/MacOS/shy-notes"
if [[ -z "$BIN" && -x "$APP_EXEC" ]]; then
  BIN="$APP_EXEC"
fi

# Sandbox / alternate target dir used by Cursor agents.
if [[ -z "$BIN" ]]; then
  while IFS= read -r path; do
    BIN="$path"
    break
  done < <(find /var/folders -path '*cargo-target/release/shy-notes' -type f -perm -111 2>/dev/null | head -1)
fi

if [[ -z "$BIN" || ! -x "$BIN" ]]; then
  echo "Building release binary for smoke test..."
  cargo build -p shy-notes --release
  BIN="$ROOT/target/release/shy-notes"
  if [[ ! -x "$BIN" && -n "${CARGO_TARGET_DIR:-}" && -x "$CARGO_TARGET_DIR/release/shy-notes" ]]; then
    BIN="$CARGO_TARGET_DIR/release/shy-notes"
  fi
fi

echo "Smoke launching: $BIN"
"$BIN" &
PID=$!
cleanup() {
  kill "$PID" 2>/dev/null || true
  wait "$PID" 2>/dev/null || true
}
trap cleanup EXIT

sleep 2
if ! kill -0 "$PID" 2>/dev/null; then
  echo "ERROR: shy-notes exited during smoke window" >&2
  exit 1
fi

echo "Smoke OK (process $PID stayed alive ~2s)"
