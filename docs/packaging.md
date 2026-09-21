# shy-notes packaging notes (Phase 8)

## Unsigned local builds

```bash
pnpm install
pnpm tauri build
```

Artifacts land under `src-tauri/target/release/bundle/` (platform-specific).

## macOS Gatekeeper (unsigned)

Until notarization credentials are available:

1. Build unsigned with `pnpm tauri build`
2. Users may need to right-click → Open, or remove quarantine:
   `xattr -dr com.apple.quarantine /path/to/shy-notes.app`

## Signing / notarization

**Deferred.** Apple Developer ID notarization and Windows Authenticode are out of scope until the maintainer unlocks certificates. Do not introduce workarounds that bypass OS security policy.

## Linux

Prefer validating on **X11**. Wayland is best-effort; document degraded evasion if global mouse / free positioning is unavailable.
