# presto

A command-line HTTP client with built-in [MPP](https://mpp.sh) payment support. Like `curl` or `wget`, but when a server requires payment,  tempo-wallethandles it automatically.

When a server responds with `402 Payment Required`,  tempo-walletdetects the [Machine Payments Protocol (MPP)](https://mpp.sh) challenge, signs a transaction on the [Tempo](https://tempo.xyz) blockchain, and retries the request — all in one step.

## Quick Start

```bash
# Install
curl -fsSL https://raw.githubusercontent.com/tempoxyz/presto/main/install.sh | bash

# Connect your wallet
 tempo-walletlogin

# Make a paid request
 tempo-wallethttps://openai.mpp.tempo.xyz/v1/chat/completions \
  -X POST --json '{"model":"gpt-4o-mini","messages":[{"role":"user","content":"Hello!"}]}'
```

## Installation

### Quick Install (macOS / Linux)

```bash
curl -fsSL https://raw.githubusercontent.com/tempoxyz/presto/main/install.sh | bash
```

### From Source

Requires [Rust](https://rustup.rs/).

```bash
git clone https://github.com/tempoxyz/presto.git
cd presto
make install
```

## Usage

```
 tempo-wallet[OPTIONS] <URL>
 tempo-wallet[OPTIONS] <COMMAND>
```

### Making Requests

Just pass a URL directly to  tempo-wallet— it works like `curl`:

```bash
# Simple GET
 tempo-wallethttps://api.example.com/data

# POST with JSON body
 tempo-wallet-X POST --json '{"key":"value"}' https://api.example.com/data

# Custom headers
 tempo-wallet-H "Accept: application/json" https://api.example.com/data

# Save response to file
 tempo-wallet-o response.json https://api.example.com/data

# Include response headers in output
 tempo-wallet-i https://api.example.com/data
```

### Payment Options

```bash
# Preview payment without executing
 tempo-wallet--dry-run https://api.example.com/data
```

### Output Control

```bash
 tempo-wallet-v <URL>          # Payment flow narration (intent, network, amount, completion)
 tempo-wallet-vv <URL>         # Debug internals (voucher retries, auth header size)
 tempo-wallet-q <URL>          # Quiet — suppress all stderr logs (overrides RUST_LOG)
 tempo-wallet--output-format json <URL>   # JSON output format
```

 tempo-walletrespects the [`NO_COLOR`](https://no-color.org/) environment variable.

## Commands

| Command | Description |
|---------|-------------|
| `<URL>` | Make an HTTP request with automatic payment |
| `login` | Sign up or log in to your Tempo wallet |
| `logout` | Log out and disconnect your wallet |
| `whoami` | Show wallet address, balances, and keys |
| `session list` | List active payment sessions |
| `session close` | Close a payment session |
| `key` or `key list` | List all keys and their spending limits |

Run ` tempo-wallet<command> --help` for detailed usage on any command.

### Version Info

` tempo-wallet--version` prints version, git commit, build date, and build profile:

```
 tempo-wallet0.4.1 (abc1234 2024-01-15T12:00:00Z release)
```

Use `-j --version` for structured JSON output with fields: `version`, `git_commit`, `build_date`, `profile`.

## Configuration

### Setup

```bash
 tempo-walletlogin    # Sign up or log in via browser
```

This creates a wallet credential file with your account address, stores your wallet EOA key securely in the OS keychain (macOS Keychain), and writes the key inline to `keys.toml` after login.

### File Locations

 tempo-walletuses platform-native directories:

| Platform | Config | Keys |
|----------|--------|--------|
| **macOS** | `~/Library/Application Support/presto/config.toml` | `~/Library/Application Support/presto/keys.toml` |
| **Linux** | `~/.config/presto/config.toml` | `~/.local/share/presto/keys.toml` |

The wallet EOA private key is stored in the OS keychain on macOS. The signing key used for payments is stored inline in `keys.toml` with permissions 0600 alongside account metadata.

### Config File Reference

```toml
# RPC overrides for built-in networks
tempo_rpc = "https://my-custom-tempo-rpc.com"
moderato_rpc = "https://my-custom-moderato-rpc.com"

# General RPC overrides (by network id)
[rpc]
tempo = "https://alternate-tempo-rpc.com"
"tempo-moderato" = "https://alternate-moderato-rpc.com"

# Telemetry (optional)
[telemetry]
enabled = true
```

Typed overrides (`tempo_rpc`, `moderato_rpc`) take precedence over the `[rpc]` table. The `PRESTO_RPC_URL` env var overrides everything.

## Examples

Ready-to-run scripts are in the [`examples/`](examples/) directory:

| Script | Description |
|--------|-------------|
| [`basic.sh`](examples/basic.sh) | Single paid request using the charge intent (one on-chain tx per request) |
| [`session-multi-fetch.sh`](examples/session-multi-fetch.sh) | Multiple requests over a single payment channel (one on-chain tx, then off-chain vouchers) |
| [`session-sse.sh`](examples/session-sse.sh) | Streaming SSE responses with per-token vouchers over a payment channel |

```bash
# Run the basic example
./examples/basic.sh

# Run with a custom prompt
./examples/basic.sh "What is the meaning of life?"
```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, project structure, and guidelines.

```bash
make build          # Debug build
make test           # Run all tests
make check          # fmt + clippy + test + build
```
