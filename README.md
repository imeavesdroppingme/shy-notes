# shy-notes

A floating desktop notepad that gently moves aside when your cursor approaches — unless you aim for its center and capture it to write.

## Features (MVP)

- Always-on-top undecorated window with plain-text editor
- Soft evasive repulsion based on cursor trajectory
- Clickless capture via a central capture surface
- Pin / unpin (tray + header control)
- Local persistence of text, position, size, and pin state
- Global shortcut: `Cmd+Shift+Space` (macOS) / `Ctrl+Shift+Space` (Windows/Linux)
- macOS Accessibility guidance dialog when permission is missing

## Stack

- Tauri 2 + Rust + TypeScript
- Pure geometry/state machine in `crates/interaction-core`

## Develop

```bash
pnpm install
pnpm tauri dev
```

Local checks:

```bash
./scripts/ci-local.sh
```

This runs unit tests, e2e cursor-session replays (`crates/interaction-core/tests/e2e_replay.rs`), frontend build, and a short binary smoke launch (`tests/e2e/smoke.sh`). Full GUI feel-testing (Accessibility + real mouse) remains manual.

## Linux notes

X11 is the supported path. Wayland is best-effort due to compositor limits on global mouse polling and arbitrary window positioning.

## Packaging

Unsigned local builds:

```bash
pnpm tauri build
```

Code signing and Apple notarization are deferred until release credentials are available.

## License

GPL-3.0-only — see [LICENSE](LICENSE).
