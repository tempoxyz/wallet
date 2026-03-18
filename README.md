# Tempo CLI Wallet Extensions

Wallet and request extensions for the Tempo CLI.

- `tempo-wallet` — wallet identity and custody (login, keys, sessions, services, signing)
- `tempo-request` — HTTP client with built-in [MPP](https://mpp.dev) payments
- `tempo-sign` — release manifest signing tool for extension distribution

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
