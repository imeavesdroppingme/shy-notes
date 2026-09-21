# ADR-001: Technology lock — Tauri 2

## Status

Accepted

## Context

shy-notes needs a cross-platform desktop shell with HTML/CSS/JS UI, always-on-top windows, programmatic position/size, global mouse sampling, multi-monitor awareness, and a small distribution footprint.

## Decision

Use **Tauri 2** with a **Rust** backend and a **vanilla TypeScript** frontend.

Evasion logic lives in a pure Rust crate `interaction-core`, independent of Tauri, so geometry and the state machine can be unit-tested without a GUI.

Fallbacks (Electron, then Qt) are allowed only if Phase 2 gates fail on an MVP OS — via a new ADR, not a silent rewrite.

## Consequences

- Smaller binary than Electron; Rust owns native APIs.
- Global mouse and Accessibility (macOS) require careful native code and user permission UX.
- Linux Wayland may degrade; see ADR-002.
- Code signing is deferred; see ADR-003.
