# tempo-sign

Release manifest signing tool for Tempo CLI extensions. Generates signed JSON manifests that authenticate build artifacts for secure distribution.

## Usage

```bash
# Generate a signing keypair
tempo-sign generate-key release.key

# Sign release artifacts
tempo-sign sign \
  --key-file release.key \
  --artifacts-dir artifacts \
  --version "0.1.0" \
  --base-url https://cli.tempo.xyz/tempo-wallet \
  --output manifest.json

# Print the public key
tempo-sign print-public-key release.key
```

Used internally by the release CI workflow.

## License

Dual-licensed under [Apache 2.0](../../LICENSE-APACHE) and [MIT](../../LICENSE-MIT).
