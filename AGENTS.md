# AGENTS.md

## Repository Overview

This is `tempo-wallet` - a pure binary crate providing a command-line HTTP client with built-in [MPP](https://mpp.dev) payment support.

**Supported Payment Protocols:**
- [Machine Payments Protocol (MPP)](https://mpp.dev) - Open protocol for HTTP-native machine-to-machine payments

### Crate Structure

Single binary crate with source organized by module directories:
- `src/main.rs` - CLI entry point, module declarations, error rendering
- `src/cli/` - CLI argument parsing and command dispatch
  - `args.rs` - clap definitions (`Cli`, `QueryArgs`, `Commands`)
  - `run.rs` - Application lifecycle: tracing, color, context building, command dispatch, analytics
  - `context.rs` - `Context` struct (Cli, Config, NetworkId, Keystore, Analytics, OutputFormat)
  - `output.rs` - `OutputFormat`, `OutputOptions`
  - `exit_codes.rs` - Process exit codes
  - `commands/` - All command implementations (take `&Context` as first arg)
    - `query/` - Query command (request → 402 → payment → response)
    - `sessions/` - Session management (list, info, close, recover, sync)
    - `wallets/` - Wallet management (create/list, fund/, keychain.rs)
    - `keys.rs` - Key listing, balance and spending limit queries
    - `login/` - Login command (mod.rs, passkey.rs)
    - `logout.rs` - Logout command
    - `whoami.rs` - Whoami command
    - `services.rs` - Service directory listing and details
    - `completions.rs` - Shell completions
    - `update.rs` - Self-update command
- `src/account/` - Wallet account types (balances, spending limits) and on-chain queries
- `src/http/` - HTTP client, request planning, response parsing
- `src/config.rs` - Configuration file handling
- `src/network.rs` - Network definitions (`NetworkId`), explorer config, RPC
- `src/keys/` - Key storage (model, I/O), signer resolution, authorization
- `src/payment/` - Payment protocol implementations
  - `dispatch.rs` - Payment dispatch (route 402 flows to charge or session)
  - `charge.rs` - One-shot on-chain charge payment
  - `session/` - Session-based payment channels (channel.rs, close.rs, store.rs, streaming.rs, tx.rs)
- `src/analytics.rs` - Opt-out telemetry (PostHog)
- `src/version.rs` - Version checking and self-update
- `src/error.rs` - Error types
- `src/util.rs` - Shared utilities (formatting, terminal hyperlinks, sanitization)
- `tests/` - Integration tests (black-box CLI testing via assert_cmd)

**Package:** `tempo-wallet` | **Binary:** `tempo-wallet`

## Commands

```bash
make build              # Build debug binary
make release            # Build optimized release binary
make test               # Run all tests (uses mocks, no network required)
make check              # Run fmt check, clippy, tests, and build (linting handled in CI)
make fix                # Auto-fix formatting and clippy warnings
make install            # Install CLI to ~/.local/bin
make uninstall          # Uninstall CLI
make run ARGS="<url>"   # Run CLI with arguments
```

## Agent Suggestions

When the user explicitly says "ask the oracle" to check a value, run `tempo-wallet` against OpenRouter and explicitly tell the user which model was used in the response.

Example:
```bash
 tempo-wallet-v -X POST --json '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"what is 1+1"}]}'  https://openrouter.mpp.tempo.xyz/v1/chat/completions | jq
```

## CRITICAL: Pre-Commit Requirements

### Before Every Commit, You MUST:

1. ✅ **Check**: `make check` - ZERO issues

## Pull Request Guidelines

When creating pull requests:

1. **Always include the PR link** in your response after creating a PR
2. Format as a clickable link: `[#123](https://github.com/tempoxyz/wallet/pull/123)`
3. When creating multiple PRs, provide a summary table with all links

Example summary format:
```
| PR | Title | Link |
|----|-------|------|
| 1 | feat: add feature X | [#123](https://github.com/tempoxyz/wallet/pull/123) |
| 2 | fix: resolve issue Y | [#124](https://github.com/tempoxyz/wallet/pull/124) |
```

## Code Style Guidelines

### Rust Conventions

- **Edition**: Rust 2021
- **Error handling**: Use `thiserror` for error types, `anyhow` for propagation
- **Async runtime**: Tokio (minimal features: macros, rt-multi-thread, signal)
- **Serialization**: Serde with derive macros

### Imports

- Group imports: std → external crates → crate modules
- Use `use crate::` for internal module imports

```rust
use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

use crate::config::Config;
use crate::error::TempoWalletError;
```

### Error Handling Pattern

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum MyError {
    #[error("failed to parse: {0}")]
    ParseError(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
```

### Module Organization

- Each module should have a clear single responsibility
- Use `mod.rs` for modules with submodules
- CLI commands go in `src/cli/commands/` (e.g., `query/`, `sessions/`, `login/`)

### Testing

- Integration tests in `tests/` use assert_cmd for black-box CLI testing
- Use `TestConfigBuilder` for setting up test configurations
- Use `test_command(&temp)` helper to create properly configured CLI commands

```rust
use crate::common::{TestConfigBuilder, test_command};

#[test]
fn test_something() {
    let temp_dir = TestConfigBuilder::new().build();
    
    let mut cmd = test_command(&temp_dir);
    cmd.arg("--help");
    cmd.assert().success();
}
```

### CLI Patterns (clap)

- Use derive macros for argument parsing
- Group related args with `help_heading`
- Support short aliases (`-v`) and long aliases (`--verbose`)
```rust
#[derive(Parser, Debug)]
pub struct Cli {
    #[arg(short = 'v', long = "verbose", action = ArgAction::Count, global = true)]
    pub verbose: u8,
}
```

## Making Changes

### Before Starting Any Code Changes:
- [ ] Check existing patterns in similar files
- [ ] Identify affected tests

### Adding New Features

1. Add core logic in appropriate module under `src/`
2. Add CLI flags in `src/cli/args.rs`, implement commands in `src/cli/`
3. Add tests: unit tests in source files, integration tests in `tests/`

## Dependencies

### Key External Crates

| Crate | Purpose |
| ----- | ------- |
| `clap` | CLI argument parsing |
| `alloy` | EVM interactions and signing (minimal features) |
| `reqwest` | HTTP client |
| `serde` / `serde_json` / `toml` | Serialization |
| `tokio` | Async runtime (minimal features) |
| `mpp` | [Machine Payments Protocol](https://mpp.dev) SDK |
| `clap-verbosity-flag` | CLI verbosity levels |

### Adding Dependencies

Add dependencies directly to `Cargo.toml`:

```toml
[dependencies]
new-crate = "1.0"
```

## Environment Variables

| Variable | Description |
| -------- | ----------- |
| `TEMPO_RPC_URL` | Override RPC endpoint |
| `TEMPO_AUTH_URL` | Override auth server URL |
| `TEMPO_NO_TELEMETRY` | Disable telemetry |
| `TEMPO_PRIVATE_KEY` | Provide a private key directly for payment (bypasses wallet login and keychain; ephemeral) |

## Data Locations

**macOS:**
- Config: `~/Library/Application Support/tempo-wallet/config.toml`
- Wallet keys: `~/Library/Application Support/tempo-wallet/keys.toml`
- Private keys: macOS Keychain

**Linux:**
- Config: `~/.config/tempo-wallet/config.toml`
- Wallet keys: `~/.local/share/tempo-wallet/keys.toml`
- Private keys: not yet supported via OS keychain (unit tests use in-memory keychain)

## Configuration Structure

```rust
struct Config {
    tempo_rpc: Option<String>,    // Typed RPC override for Tempo mainnet
    moderato_rpc: Option<String>, // Typed RPC override for Moderato testnet
    rpc: HashMap<String, String>, // General RPC overrides by network id
}
```

**Wallet Fields (`keys.toml`):**
- `wallet_type` — `"local"` or `"passkey"`
- `wallet_address` — On-chain wallet address (the fundable address)
- `chain_id` — Chain ID this key is authorized for
- `key_type` — Signature type (`"secp256k1"`, `"p256"`, or `"webauthn"`)
- `key_address` — Address of the signing key
- `key` — Signing key private key stored inline; file is written with mode 0600
- `key_authorization` — RLP-encoded on-chain authorization proof for this key
- `expiry` — Unix timestamp for key authorization expiry
- `limits` — Array of `{ currency: "0x...", limit: "..." }`
- `provisioned` — Whether this key has been provisioned on-chain

**Key Selection:** Deterministic: passkey > first key with `key` > first key (lexicographically). The old `active` field was removed.

**Network Resolution Priority:**
1. `TEMPO_RPC_URL` env var (overrides everything)
2. Typed overrides (`tempo_rpc`, `moderato_rpc`) take precedence
3. General `[rpc]` table as fallback
4. Default RPC if no override

**Built-in Networks:** `tempo` (chain 4217, mainnet), `tempo-moderato` (chain 42431, testnet)

**Built-in Tokens:** USDC (mainnet), pathUSD (testnet)

## Documentation

- [Rust Book](https://doc.rust-lang.org/book/)
- [Alloy Documentation](https://alloy.rs/)
