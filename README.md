# Tempo CLI

A command-line toolkit for the [Tempo](https://tempo.xyz) blockchain. Call APIs without API keys — pay per request automatically using the [Machine Payments Protocol](https://mpp.dev).

- **No API keys** — pay per request, skip signups and billing dashboards
- **No minimums** — pay only for what you use, down to fractions of a cent
- **curl-compatible** — familiar flags (`-X`, `-H`, `--json`, `-o`, `-L`, …)
- **Payment sessions** — open a channel once, then pay per-request off-chain
- **Dry-run** — preview cost before committing (`--dry-run`)

## Install

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
tempo mpp https://openrouter.mpp.tempo.xyz/v1/chat/completions \
  -X POST --json '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"Hello!"}]}'
```

## Examples

Chat with an LLM:

```bash
tempo mpp https://openrouter.mpp.tempo.xyz/v1/chat/completions \
  -X POST --json '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"Hello!"}]}'
```

Generate an image:

```bash
tempo mpp https://fal.mpp.tempo.xyz/fal-ai/flux/schnell \
  -X POST --json '{"prompt":"A golden retriever in a sunny park","image_size":"landscape_4_3"}'
```

Preview cost without paying:

```bash
tempo mpp --dry-run https://openrouter.mpp.tempo.xyz/v1/chat/completions \
  -X POST --json '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"Hello!"}]}'
```

## Commands

| Command | Description |
|---------|-------------|
| `tempo wallet login` | Connect your Tempo wallet |
| `tempo wallet logout` | Disconnect your wallet |
| `tempo wallet whoami` | Show wallet, balances, and keys |
| `tempo mpp <URL>` | Make an HTTP request with automatic payment |
| `tempo mpp services` | Browse the MPP service directory |
| `tempo mpp services info <ID>` | Show detailed info for a service |
| `tempo mpp sessions list` | List sessions (active/orphaned/closing) |
| `tempo mpp sessions close [--all\|<URL>]` | Close sessions or channels |
| `tempo mpp sign` | Sign an MPP challenge and output the Authorization header |

Run `tempo wallet --help` or `tempo mpp --help` for full flag reference.

## Workspace

| Crate | Binary | Description |
|-------|--------|-------------|
| `tempo-wallet` | `tempo-wallet` | Wallet identity and custody (login, keys, fund) |
| `tempo-mpp` | `tempo-mpp` | HTTP client with MPP payment (query, sessions, services) |
| `tempo-common` | — | Shared library (config, keys, network, payment, analytics) |
| `tempo-sign` | `tempo-sign` | Release manifest signing tool |

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
