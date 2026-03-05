# AGENTS.md

## Repository Overview

This is `tempo-wallet` - a pure binary crate providing a command-line HTTP client with built-in [MPP](https://mpp.dev) payment support.

**Supported Payment Protocols:**
- [Machine Payments Protocol (MPP)](https://mpp.dev) - Open protocol for HTTP-native machine-to-machine payments

### Crate Structure

Single binary crate with source organized by module directories:
- `src/main.rs` - CLI entry point and module declarations
- `src/cli/` - CLI argument parsing and all command implementations
  - `args.rs` - clap definitions (`Cli`, `QueryArgs`, `Commands`)
  - `query.rs` - Query command (request → 402 → payment → response)
  - `auth.rs` - Login, logout, whoami commands
  - `keys.rs` - Key listing, balance and spending limit queries
  - `local_wallet.rs` - Local wallet management (create/import/delete)
  - `session/` - Session management commands (directory module with list.rs, info.rs, close.rs, recover.rs, render.rs, sync.rs)
  - `output.rs` - Response display, `OutputOptions`
  - `exit_codes.rs` - Process exit codes
  - `fund.rs` - Wallet funding (testnet faucet, mainnet bridge via Relay)
  - `relay.rs` - Relay bridge client for cross-chain wallet funding
  - `services.rs` - Service directory listing and details
- `src/http.rs` - HTTP client, `RequestContext`, `RequestRuntime`
- `src/config.rs` - Configuration file handling
- `src/network.rs` - Network definitions, explorer config, RPC
- `src/payment/` - Payment protocol implementations
  - `charge.rs` - One-shot on-chain charge payment
  - `session/` - Session-based payment channels (directory module with channel.rs, close.rs, store.rs, streaming.rs, tx.rs)
- `src/wallet/` - Wallet management and signing
  - `credentials/` - Credential storage and key management (directory module with model.rs, io.rs, overrides.rs)
  - `key_authorization.rs` - Key authorization decode/validate/sign
  - `keychain.rs` - Platform-native secret storage (macOS Keychain)
  - `passkey.rs` - Browser-based passkey wallet flow
  - `signer.rs` - Signing mode resolution
- `src/analytics/` - Opt-out telemetry (PostHog)
- `src/util.rs` - Shared utilities (atomic writes, terminal hyperlinks)
- `src/error.rs` - Error types
- `src/services/` - MPP service directory (registry fetching, data model)
- `tests/` - Integration tests (black-box CLI testing via assert_cmd)

**Package:** `tempo-wallet` | **Binary:** `tempo-wallet`

## Commands

```bash
make build              # Build debug binary
make release            # Build optimized release binary
make test               # Run all tests (uses mocks, no network required)
make check              # Run fmt check, clippy, tests, and build (linting handled in CI)
make fix                # Auto-fix formatting and clippy warnings
make install            # Install CLI to /usr/local/bin
make uninstall          # Uninstall CLI
make reinstall          # Rebuild and reinstall CLI
make run ARGS="<url>"   # Run CLI with arguments
```

## Agent Suggestions

When the user explicitly says "ask the oracle" to check a value, run `tempo-wallet` against OpenRouter and explicitly tell the user which model was used in the response.

Example:
```bash
tempo-wallet -v -X POST --json '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"what is 1+1"}]}'  https://openrouter.mpp.tempo.xyz/v1/chat/completions | jq
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
- CLI commands go in `src/cli/` (e.g., `query.rs`, `auth.rs`, `session/`)

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
    #[command(flatten)]
    pub verbose: clap_verbosity_flag::Verbosity<clap_verbosity_flag::WarnLevel>,
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
| `TEMPO_WALLET_TYPE` | Set to `"local"` to default to local wallet mode (affects `tempo-wallet login`/`tempo-wallet wallet create` guidance). When unset, passkey mode is the default. Does **not** select which wallet to use at runtime — wallet selection is determined by the credentials in `keys.toml`. Multi-wallet support: the first matching key entry is used (passkey > first key with inline `key` > first key by address). |

## Data Locations

**macOS:**
- Config: `~/Library/Application Support/tempo-wallet/config.toml`
- Wallet credentials: `~/Library/Application Support/tempo-wallet/keys.toml`
- Private keys: macOS Keychain

**Linux:**
- Config: `~/.config/tempo-wallet/config.toml`
- Wallet credentials: `~/.local/share/tempo-wallet/keys.toml`
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
- `token_limits` — Array of `{ currency: "0x...", limit: "..." }`
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
