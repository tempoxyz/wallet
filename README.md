# presto

A command-line HTTP client with built-in [MPP](https://mpp.sh) payment support. Like `curl` or `wget`, but when a server requires payment, presto handles it automatically.

When a server responds with `402 Payment Required`, presto detects the [Machine Payments Protocol (MPP)](https://mpp.sh) challenge, signs a transaction on the [Tempo](https://tempo.xyz) blockchain, and retries the request — all in one step.

## Quick Start

```bash
# Install
curl -fsSL https://raw.githubusercontent.com/tempoxyz/presto/main/install.sh | bash

# Connect your wallet
presto login

# Make a paid request
presto https://openai.mpp.tempo.xyz/v1/chat/completions \
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
cargo install --path .
```

Make sure `~/.cargo/bin` is on your `PATH`:

```bash
export PATH="$HOME/.cargo/bin:$PATH"
```

## Usage

```
presto [OPTIONS] <URL>
presto [OPTIONS] <COMMAND>
```

### Making Requests

Just pass a URL directly to presto — it works like `curl`:

```bash
# Simple GET
presto https://api.example.com/data

# POST with JSON body
presto -X POST --json '{"key":"value"}' https://api.example.com/data

# Custom headers
presto -H "Accept: application/json" https://api.example.com/data

# Save response to file
presto -o response.json https://api.example.com/data

# Include response headers in output
presto -i https://api.example.com/data
```

### Payment Options

```bash
# Preview payment without executing
presto --dry-run https://api.example.com/data

# Restrict to a specific network
presto -n tempo https://api.example.com/data
```

### Output Control

```bash
presto -v <URL>          # Verbose output
presto -vv <URL>         # Debug output
presto -q <URL>          # Quiet — suppress logs
presto --color never <URL>          # Disable colors
presto --output-format json <URL>   # JSON output format
```

presto respects the [`NO_COLOR`](https://no-color.org/) environment variable.

## Commands

| Command | Description |
|---------|-------------|
| `<URL>` | Make an HTTP request with automatic payment |
| `login` | Sign up or log in to your Tempo wallet |
| `logout` | Log out and disconnect your wallet |
| `whoami` | Show wallet address, balances, and access keys |
| `session list` | List active payment sessions |
| `session close` | Close a payment session |
| `session recover` | Recover a session from on-chain state |

Run `presto <command> --help` for detailed usage on any command.

### Wallet Commands

```bash
# Create a new local wallet (EOA stored in macOS Keychain)
presto wallet create --name default

# Import an existing private key as a local wallet
presto wallet import --name default --stdin-key   # read from stdin
presto wallet import --name default --private-key 0x...

# Delete a wallet
presto wallet delete --name default --yes
```

## Configuration

### Setup

```bash
presto login    # Sign up or log in via browser
```

This creates a wallet credential file with your account address, stores your wallet EOA key securely in the OS keychain (macOS Keychain), and writes the access key inline to `keys.toml` after login.

### File Locations

presto uses platform-native directories:

| Platform | Config | Keys |
|----------|--------|--------|
| **macOS** | `~/Library/Application Support/presto/config.toml` | `~/Library/Application Support/presto/keys.toml` |
| **Linux** | `~/.config/presto/config.toml` | `~/.local/share/presto/keys.toml` |

The wallet EOA private key is stored in the OS keychain on macOS. The access key used for payments is stored inline in `keys.toml` with permissions 0600 alongside account metadata.

You can override the config path with `-c <PATH>` or `--config <PATH>`.

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
