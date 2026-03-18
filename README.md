# Tempo CLI Wallet Extensions

This repository provides the **wallet extension binaries** for the [Tempo CLI](https://github.com/tempoxyz/tempo). The main `tempo` launcher (at `tempoxyz/tempo`) discovers, installs, and manages these extensions automatically — you don't need to build this repo unless you're contributing.

- **`tempo-wallet`** — Wallet identity and custody: login, key management, sessions, services, signing
- **`tempo-request`** — HTTP client with built-in [MPP](https://mpp.dev) payment: make API requests without API keys

Both extensions are built on the [Machine Payments Protocol](https://mpp.dev), an open protocol for HTTP-native machine-to-machine payments on the [Tempo](https://tempo.xyz) blockchain.

## Install

```bash
curl -fsSL https://tempo.xyz/install | bash
```

## Quick Start

```bash
# Connect your wallet
tempo wallet login

# Preview a paid request without sending payment
tempo request --dry-run -X POST \
  --json '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"Hello!"}]}' \
  https://openrouter.mpp.tempo.xyz/v1/chat/completions
```

## Workspace Overview

| Crate | Binary | Purpose |
|-------|--------|---------|
| `tempo-common` | — | Shared config, keys, network, payment, and CLI runtime |
| `tempo-wallet` | `tempo-wallet` | Wallet login, funding, services, sessions, and signing |
| `tempo-request` | `tempo-request` | HTTP client with automatic `402 Payment Required` handling |
| `tempo-sign` | `tempo-sign` | Release artifact manifest signing |

Architecture details: [ARCHITECTURE.md](ARCHITECTURE.md)

## Data And Configuration

All runtime data is stored under `$TEMPO_HOME` (defaults to `~/.tempo`):

| File | Path | Description |
|------|------|-------------|
| Config | `~/.tempo/config.toml` | RPC overrides and telemetry settings |
| Keys | `~/.tempo/wallet/keys.toml` | Wallet keys (mode 0600) |
| Channels | `~/.tempo/wallet/channels.db` | Persisted payment channel state |

## Telemetry

Anonymous usage analytics (PostHog) are enabled by default. Request bodies, API keys, and wallet private keys are not collected.
Telemetry events are only sent when `POSTHOG_API_KEY` is set (either at build time in CI or as a runtime environment variable).

Disable telemetry:

```bash
export TEMPO_NO_TELEMETRY=1
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for setup and workflow.

```bash
make build
make test
make check
```

## Security

See [SECURITY.md](SECURITY.md) for vulnerability reporting.

## License

Dual-licensed under [Apache 2.0](LICENSE-APACHE) and [MIT](LICENSE-MIT).
