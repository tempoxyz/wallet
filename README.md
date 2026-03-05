# tempo-wallet

 tempo-walletis a command-line HTTP client that pays for API calls automatically. Call services without signing up or managing API keys —  tempo-wallethandles payment on the [Tempo](https://tempo.xyz) blockchain using the [Machine Payments Protocol](https://mpp.dev).

- **No API keys** — pay per request, skip signups and billing dashboards
- **No minimums** — pay only for what you use, down to fractions of a cent
- **curl-compatible** — familiar flags (`-X`, `-H`, `--json`, `-o`, `-L`, …)
- **Payment sessions** — open a channel once, then pay per-request off-chain
- **Dry-run** — preview cost before committing (`--dry-run`)

## Quick Start

```bash
# Install
curl -fsSL cli.tempo.xyz/install.sh | bash

# Connect your wallet
 tempo-walletlogin

# Make a paid API request
 tempo-wallethttps://openrouter.mpp.tempo.xyz/v1/chat/completions \
  -X POST --json '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"Hello!"}]}'
```

### From Source

```bash
git clone https://github.com/tempoxyz/wallet.git
cd  tempo-wallet&& make install
```

## Examples

Chat with an LLM:

```bash
 tempo-wallethttps://openrouter.mpp.tempo.xyz/v1/chat/completions \
  -X POST --json '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"Hello!"}]}'
```

Generate an image:

```bash
 tempo-wallethttps://fal.mpp.tempo.xyz/fal-ai/flux/schnell \
  -X POST --json '{"prompt":"A golden retriever in a sunny park","image_size":"landscape_4_3"}'
```

Preview cost without paying:

```bash
 tempo-wallet--dry-run https://openrouter.mpp.tempo.xyz/v1/chat/completions \
  -X POST --json '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"Hello!"}]}'
```

Ready-to-run scripts in [`examples/`](examples/):
[`basic.sh`](examples/basic.sh) · [`session-multi-fetch.sh`](examples/session-multi-fetch.sh) · [`session-sse.sh`](examples/session-sse.sh)

## Commands

| Command | Description |
|---------|-------------|
| ` tempo-wallet<URL>` | Make an HTTP request with automatic payment |
| ` tempo-walletlogin` | Connect your Tempo wallet |
| ` tempo-walletlogout` | Disconnect your wallet |
| ` tempo-walletwhoami` | Show wallet, balances, and keys |
| ` tempo-walletservices` | Browse the MPP service directory |
| ` tempo-walletservices info <ID>` | Show detailed info for a service |
| ` tempo-walletupdate` | Update  tempo-walletto the latest version |
| ` tempo-walletsessions list` | List sessions (active/orphaned/closing) |
| ` tempo-walletsessions info <URL|channel_id>` | Show details for a session or channel |
| ` tempo-walletsessions close [--all|--orphaned|--closed|<URL>|<channel_id>]` | Close sessions or channels |
| ` tempo-walletsessions recover <URL|origin>` | Re-sync a local session's state from chain |
| ` tempo-walletsessions sync` | Remove stale local sessions (settled on-chain) |

Run ` tempo-wallet--help` or ` tempo-wallet<command> --help` for full flag reference.

## Configuration

```bash
 tempo-walletlogin    # Opens browser to create or connect a passkey wallet
```

Credentials are stored in `keys.toml` (signing key inline, permissions `0600`).

| Platform | Config | Keys |
|----------|--------|------|
| **macOS** | `~/Library/Application Support/tempo/wallet/config.toml` | `~/Library/Application Support/tempo/wallet/keys.toml` |
| **Linux** | `~/.config/tempo/wallet/config.toml` | `~/.local/share/tempo/wallet/keys.toml` |

## Telemetry

 tempo-walletcollects anonymous usage analytics (via PostHog) to help improve the tool. No personal data, API keys, request bodies, or wallet private keys are ever collected.

Opt out with:

```bash
export TEMPO_NO_TELEMETRY=1 
```

Or disable in `config.toml`:

```toml
[telemetry]
enabled = false
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, project structure, and guidelines.

```bash
make build    # Debug build
make test     # Run all tests
make check    # fmt + clippy + test + build
```

## License

Dual-licensed under [Apache 2.0](LICENSE-APACHE) and [MIT](LICENSE-MIT).
