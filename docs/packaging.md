# shy-notes packaging notes

## Local unsigned builds

```bash
pnpm install
pnpm tauri build
```

Artifacts land under `src-tauri/target/release/bundle/` (platform-specific).

## GitHub Actions (recommended)

Cross-platform bundles are produced by [`.github/workflows/release.yml`](../.github/workflows/release.yml):

| Platform | Runner | Typical assets |
| -------- | ------ | -------------- |
| macOS Apple Silicon | `macos-latest` | `.app` / `.dmg` (aarch64) |
| macOS Intel | `macos-latest` | `.app` / `.dmg` (x86_64) |
| Windows | `windows-latest` | NSIS `.exe` / MSI |
| Linux (Debian/Ubuntu) | `ubuntu-22.04` | `.deb` (+ AppImage if enabled) |

### How to run

1. Repo **Settings → Actions → General → Workflow permissions** → enable **Read and write** (so the workflow can create a release).
2. Either:
   - **Actions → Release → Run workflow**, or
   - `git tag v0.1.0 && git push origin v0.1.0`
3. Open the **draft** release on GitHub, download assets, then publish when ready.

Builds are **unsigned** (no Apple/Windows certificates). That is intentional for now.

## macOS Gatekeeper (unsigned)

Ad-hoc signing (`signingIdentity: "-"`) is configured so Apple Silicon downloads from GitHub are less likely to show as “damaged”.

Users may still need right-click → Open, or:

```bash
xattr -dr com.apple.quarantine /path/to/Shy\ notes.app
```

## Signing / notarization

**Deferred.** Apple Developer ID notarization and Windows Authenticode stay out of scope until certificates are available.

## Linux

Prefer validating on **X11**. Wayland is best-effort; global mouse / free positioning may be limited by the compositor.
