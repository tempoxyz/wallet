# presto

A command-line HTTP client with built-in [MPP](https://mpp.dev) payment support.
Like `curl`, but when a server requires payment, presto handles it automatically.

## Features

- **curl-like syntax** — familiar flags (`-X`, `-H`, `--json`, `-o`, `-i`, `-L`, …)
- **Automatic payments** — detects `402 Payment Required`, pays via [Tempo](https://tempo.xyz), retries
- **Payment sessions** — open a channel once, then pay per-request with off-chain vouchers
- **Dry-run** — preview what you'd pay before committing (`--dry-run`)
- **JSON output** — structured errors and responses for scripting (`-j`)

## Quick Start

```bash
# Install
curl -fsSL https://raw.githubusercontent.com/tempoxyz/presto/main/install.sh | bash

# Connect your wallet
presto login

# Make a paid API request
presto https://openai.mpp.tempo.xyz/v1/chat/completions \
  -X POST --json '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"Hello!"}]}'
```

### From Source

```bash
git clone https://github.com/tempoxyz/presto.git
cd presto && make install
```

## Examples

Chat with an LLM:

```bash
presto https://openrouter.mpp.tempo.xyz/v1/chat/completions \
  -X POST --json '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"Hello!"}]}'
```

Generate an image:

```bash
presto https://fal.mpp.tempo.xyz/fal-ai/flux/schnell \
  -X POST --json '{"prompt":"A golden retriever in a sunny park","image_size":"landscape_4_3"}'
```

Preview cost without paying:

```bash
presto --dry-run https://openrouter.mpp.tempo.xyz/v1/chat/completions \
  -X POST --json '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"Hello!"}]}'
```

Ready-to-run scripts in [`examples/`](examples/):
[`basic.sh`](examples/basic.sh) · [`session-multi-fetch.sh`](examples/session-multi-fetch.sh) · [`session-sse.sh`](examples/session-sse.sh)

## Commands

| Command | Description |
|---------|-------------|
| `presto <URL>` | Make an HTTP request with automatic payment |
| `presto login` | Connect your Tempo wallet |
| `presto logout` | Disconnect your wallet |
| `presto whoami` | Show wallet, balances, and keys |
| `presto session list` | List active payment sessions |
| `presto session close` | Close a payment session |

Run `presto --help` or `presto <command> --help` for full flag reference.

## Configuration

```bash
presto login    # Opens browser to create or connect a passkey wallet
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
