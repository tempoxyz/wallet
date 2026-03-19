<br>
<br>

<p align="center">
  <a href="https://tempo.xyz">
    <picture>
      <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/tempoxyz/.github/refs/heads/main/assets/combomark-dark.svg">
      <img alt="tempo combomark" src="https://raw.githubusercontent.com/tempoxyz/.github/refs/heads/main/assets/combomark-bright.svg" width="auto" height="120">
    </picture>
  </a>
</p>

<br>
<br>

# Tempo Wallet

[![CI status](https://github.com/tempoxyz/wallet/actions/workflows/ci.yml/badge.svg)](https://github.com/tempoxyz/wallet/actions/workflows/ci.yml)

**Command-line wallet and HTTP client for the [Tempo](https://tempo.xyz) blockchain, with built-in [Machine Payments Protocol](https://mpp.dev) support.**

**[Install](https://wallet.tempo.xyz)**
| [Docs](https://docs.tempo.xyz)
| [MPP Spec](https://mpp.dev)
| [Architecture](ARCHITECTURE.md)

## What is Tempo Wallet?

Tempo Wallet is a CLI that lets you create a wallet, manage keys, and make HTTP requests that pay automatically — no API keys required. It uses the [Machine Payments Protocol (MPP)](https://mpp.dev) to handle `402 Payment Required` challenges natively, turning any paid API into a simple HTTP call.

The wallet supports two authentication modes:

- **Passkey (WebAuthn)** — Browser-based login via [wallet.tempo.xyz](https://wallet.tempo.xyz). Your passkey creates a smart wallet on Tempo; the CLI receives an authorized session key. No seed phrases, no private key management.
- **Local key** — A locally generated secp256k1 private key for headless or automated environments.

### How the OAuth Flow Works

1. Run `tempo wallet login` — the CLI opens your browser to [wallet.tempo.xyz](https://wallet.tempo.xyz).
2. Authenticate with your passkey (Touch ID, Face ID, or hardware key).
3. The browser authorizes a session key for the CLI and redirects back.
4. The CLI stores the authorized key locally. All subsequent signing happens locally — no browser needed.

## Goals

1. **Zero-config payments**: `tempo request <url>` handles the full 402 flow — challenge, sign, pay, retry — in a single command.
2. **Secure by default**: Passkey login means no seed phrases. Local keys are stored with mode `0600`. Private keys never leave the machine.
3. **Composable**: Both `tempo-wallet` and `tempo-request` are standalone binaries that the [`tempo` CLI](https://github.com/tempoxyz/tempo) discovers as extensions. Use them independently or together.
4. **Streaming-native**: Session-based payments support SSE streaming with per-token voucher top-ups — pay only for what you consume.

## Install

```bash
curl -fsSL https://tempo.xyz/install | bash
```

This installs the `tempo` launcher, which automatically manages wallet extensions.

## Quick Start

```bash
# Log in with your passkey (opens browser)
tempo wallet login

# Check your wallet
tempo wallet whoami

# Fund your wallet
tempo wallet fund

# Make a paid API request
tempo request -X POST \
  --json '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"Hello!"}]}' \
  https://openrouter.mpp.tempo.xyz/v1/chat/completions

# Preview a request without paying (dry run)
tempo request --dry-run https://openrouter.mpp.tempo.xyz/v1/chat/completions
```

## Workspace

| Crate | Binary | Purpose |
|-------|--------|---------|
| [`tempo-common`](crates/tempo-common/) | — | Shared library: config, keys, networking, payment, CLI runtime |
| [`tempo-wallet`](crates/tempo-wallet/) | `tempo-wallet` | Wallet login, identity, funding, sessions, services, signing |
| [`tempo-request`](crates/tempo-request/) | `tempo-request` | HTTP client with automatic `402 Payment Required` handling |
| [`tempo-sign`](crates/tempo-sign/) | `tempo-sign` | Release artifact manifest signing |

See [ARCHITECTURE.md](ARCHITECTURE.md) for crate layering, payment flows, and design decisions.

## Configuration

All data lives under `$TEMPO_HOME` (default: `~/.tempo`):

```
~/.tempo/
├── config.toml              # RPC overrides, telemetry settings
└── wallet/
    ├── keys.toml             # Wallet keys (mode 0600)
    └── channels.db           # Payment channel state (SQLite)
```

| Environment Variable | Description |
|---------------------|-------------|
| `TEMPO_HOME` | Override data directory (default: `~/.tempo`) |
| `TEMPO_RPC_URL` | Override RPC endpoint |
| `TEMPO_PRIVATE_KEY` | Ephemeral private key for payment (bypasses login) |
| `TEMPO_NO_TELEMETRY` | Disable anonymous telemetry |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for setup and workflow.

```bash
make build    # Build debug binaries
make test     # Run all tests
make check    # Format, clippy, test, docs
```

The Minimum Supported Rust Version (MSRV) is specified in [`Cargo.toml`](Cargo.toml).

## Security

See [SECURITY.md](SECURITY.md) for vulnerability reporting.

## License

Dual-licensed under [Apache 2.0](LICENSE-APACHE) and [MIT](LICENSE-MIT).
