# ADR-002: Linux display server policy

## Status

Accepted

## Context

Wayland restricts arbitrary global window positioning and reliable global mouse polling compared to X11.

## Decision

- **X11**: supported path for the Linux MVP.
- **Wayland**: best-effort. If global mouse or free positioning is unavailable, degrade evasion gracefully and surface status in docs/tray — do not fight the compositor with fragile hacks.

## Consequences

Linux CI and docs must state X11 as the validated target. Wayland users may see reduced evasive behavior.
