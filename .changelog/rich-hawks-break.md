---
tempo-wallet: minor
---

Added Ed25519 release signing support. Introduced a `sign-release` binary that generates a signed manifest of release artifacts, and updated the release workflow to sign binaries, upload the manifest to R2, and store artifacts under a versioned `extensions/tempo-wallet/` path.
