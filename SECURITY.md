# Security Policy

## Reporting A Vulnerability

If you believe you've found a security vulnerability, please do not report it via GitHub issues.
Instead, email `security@tempo.xyz` and we will acknowledge your report within 5 days and provide a more detailed follow-up within 10 days.

## Verifying Releases

Each binary ships with three sidecars: `.sha256` (checksum), `.sigstore.json`
(cosign keyless bundle), and `.spdx.json` (SBOM). GitHub also stores SLSA v1
provenance and SBOM attestations against the binary's digest. Signing identity
is the OIDC token of [`build.yml`](./.github/workflows/build.yml).

```bash
TAG=v1.0.0
PKG=tempo-wallet
BIN=${PKG}-linux-amd64
```

### From GitHub Releases

```bash
gh release download "$TAG" --repo tempoxyz/wallet \
  -p "$BIN" -p "$BIN.sha256" -p "$BIN.sigstore.json" -p "$BIN.spdx.json"

sha256sum -c "$BIN.sha256"

gh attestation verify "$BIN" --repo tempoxyz/wallet \
  --signer-workflow tempoxyz/wallet/.github/workflows/build.yml \
  --source-ref "refs/tags/${TAG}" \
  --predicate-type https://slsa.dev/provenance/v1

gh attestation verify "$BIN" --repo tempoxyz/wallet \
  --signer-workflow tempoxyz/wallet/.github/workflows/build.yml \
  --source-ref "refs/tags/${TAG}" \
  --predicate-type https://spdx.dev/Document/v2.3
```

### From cli.tempo.xyz (offline-friendly)

```bash
BASE=https://cli.tempo.xyz/extensions/${PKG}
curl -fsSL -O "${BASE}/${BIN}"
curl -fsSL -O "${BASE}/${BIN}.sha256"
curl -fsSL -O "${BASE}/${BIN}.sigstore.json"

sha256sum -c "${BIN}.sha256"

cosign verify-blob \
  --bundle "${BIN}.sigstore.json" \
  --certificate-identity "https://github.com/tempoxyz/wallet/.github/workflows/build.yml@refs/tags/${TAG}" \
  --certificate-oidc-issuer 'https://token.actions.githubusercontent.com' \
  "$BIN"
```
