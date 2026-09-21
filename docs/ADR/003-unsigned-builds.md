# ADR-003: Unsigned builds until Phase 8

## Status

Accepted

## Context

Apple notarization and Windows Authenticode require paid developer certificates and tooling.

## Decision

Ship **unsigned** local (and optional free-tier CI) builds through development. Signing and notarization happen only when Phase 8 release credentials are unlocked by the maintainer.

## Consequences

macOS Gatekeeper will warn on unsigned apps; document the expected developer workaround (right-click Open / remove quarantine) in packaging notes.
