# presto

 tempo-walletis a command-line HTTP client that pays for API calls automatically. Call services without signing up or managing API keys —  tempo-wallethandles payment on the [Tempo](https://tempo.xyz) blockchain using the [Machine Payments Protocol](https://mpp.dev).

- **No API keys** — pay per request, skip signups and billing dashboards
- **No minimums** — pay only for what you use, down to fractions of a cent
- **curl-compatible** — familiar flags (`-X`, `-H`, `--json`, `-o`, `-L`, …)
- **Payment sessions** — open a channel once, then pay per-request off-chain
- **Dry-run** — preview cost before committing (`--dry-run`)

## Quick Start

```bash
# Install
curl -fsSL https://raw.githubusercontent.com/tempoxyz/presto/main/install.sh | bash

# Connect your wallet
 tempo-walletlogin

# Make a paid API request
 tempo-wallethttps://openai.mpp.tempo.xyz/v1/chat/completions \
  -X POST --json '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"Hello!"}]}'
```

### From Source

```bash
git clone https://github.com/tempoxyz/presto.git
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
| ` tempo-walletsession list` | List active payment sessions |
| ` tempo-walletsession close` | Close a payment session |

Run ` tempo-wallet--help` or ` tempo-wallet<command> --help` for full flag reference.

## Configuration

```bash
 tempo-walletlogin    # Opens browser to create or connect a passkey wallet
```

Credentials are stored in `keys.toml` (signing key inline, permissions `0600`).

| Platform | Config | Keys |
|----------|--------|------|
| **macOS** | `~/Library/Application Support/presto/config.toml` | `~/Library/Application Support/presto/keys.toml` |
| **Linux** | `~/.config/presto/config.toml` | `~/.local/share/presto/keys.toml` |

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, project structure, and guidelines.

```bash
make build    # Debug build
make test     # Run all tests
make check    # fmt + clippy + test + build
```
