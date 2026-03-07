# tempo-cli

Top-level launcher for the Tempo CLI. Dispatches commands to extension binaries (`tempo-wallet`, `tempo-mpp`) and manages extension installation.

## Usage

```bash
# Install
curl -fsSL https://cli.tempo.xyz/install | bash

# Dispatch to extensions
tempo wallet login
tempo wallet https://example.com/api
```

The `tempo` binary looks for `tempo-<extension>` binaries on `$PATH` and forwards arguments.

## License

Dual-licensed under [Apache 2.0](../../LICENSE-APACHE) and [MIT](../../LICENSE-MIT).
