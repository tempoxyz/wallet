# Tempo CLI — Wallet Extensions

This repository provides the **wallet extension binaries** for the [Tempo CLI](https://github.com/tempoxyz/tempo). The main `tempo` launcher (at `tempoxyz/tempo`) discovers, installs, and manages these extensions automatically — you don't need to build this repo unless you're contributing.

- **`tempo-wallet`** — Wallet identity and custody: login, key management, sessions, services, signing
- **`tempo-request`** — HTTP client with built-in [MPP](https://mpp.dev) payment: make API requests without API keys

Both extensions are built on the [Machine Payments Protocol](https://mpp.dev), an open protocol for HTTP-native machine-to-machine payments on the [Tempo](https://tempo.xyz) blockchain.

## Install

The recommended way to install is via the Tempo CLI, which handles extension discovery and updates:

```bash
curl -fsSL https://cli.tempo.xyz/install | bash
```

### From Source

```bash
git clone https://github.com/tempoxyz/wallet.git
cd wallet && make install
```

## Quick Start

```bash
# Connect your wallet
tempo wallet login

# Make a paid API request
tempo request https://openrouter.mpp.tempo.xyz/v1/chat/completions \
  -X POST --json '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"Hello!"}]}'
```

## Examples

Chat with an LLM:

```bash
tempo request https://openrouter.mpp.tempo.xyz/v1/chat/completions \
  -X POST --json '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"Hello!"}]}'
```

Generate an image:

```bash
tempo request https://fal.mpp.tempo.xyz/fal-ai/flux/schnell \
  -X POST --json '{"prompt":"A golden retriever in a sunny park","image_size":"landscape_4_3"}'
```

Preview cost without paying:

```bash
tempo request --dry-run https://openrouter.mpp.tempo.xyz/v1/chat/completions \
  -X POST --json '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"Hello!"}]}'
```

## Commands

| Command | Description |
|---------|-------------|
| `tempo wallet login` | Connect your Tempo wallet |
| `tempo wallet logout` | Disconnect your wallet |
| `tempo wallet whoami` | Show wallet, balances, and keys |
| `tempo wallet services` | Browse the MPP service directory |
| `tempo wallet services info <ID>` | Show detailed info for a service |
| `tempo wallet sessions list` | List sessions (active/orphaned/closing) |
| `tempo wallet sessions close [--all\|<URL>]` | Close sessions or channels |
| `tempo wallet sign` | Sign an MPP challenge and output the Authorization header |

Run `tempo wallet --help` or `tempo request --help` for full flag reference.

## Workspace

| Crate | Binary | Description |
|-------|--------|-------------|
| `tempo-common` | — | Shared library (config, keys, network, payment, CLI infrastructure) |
| `tempo-wallet` | `tempo-wallet` | Wallet identity, custody, sessions, services, and signing |
| `tempo-request` | `tempo-request` | HTTP client with MPP payment |
| `tempo-sign` | `tempo-sign` | Release manifest signing tool |

The `tempo` launcher at [tempoxyz/tempo](https://github.com/tempoxyz/tempo) discovers these extension binaries via signed release manifests. See [ARCHITECTURE.md](ARCHITECTURE.md) for crate layering and module details.

## Configuration

```bash
tempo wallet login    # Opens browser to create or connect a passkey wallet
```

| Platform | Config | Keys |
|----------|--------|------|
| **macOS** | `~/Library/Application Support/tempo/wallet/config.toml` | `~/Library/Application Support/tempo/wallet/keys.toml` |
| **Linux** | `~/.config/tempo/wallet/config.toml` | `~/.local/share/tempo/wallet/keys.toml` |

## Telemetry

Anonymous usage analytics (via PostHog) help improve the tool. No personal data, API keys, request bodies, or wallet private keys are ever collected.

Opt out:

```bash
export TEMPO_NO_TELEMETRY=1
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, project structure, and guidelines.

```bash
make build    # Debug build
make test     # Run all tests
make check    # fmt + clippy + test + doc
```

## License

Dual-licensed under [Apache 2.0](LICENSE-APACHE) and [MIT](LICENSE-MIT).
