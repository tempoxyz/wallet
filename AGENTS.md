# AGENTS.md

## Repository Overview

This is a Cargo workspace containing 4 crates under `crates/`, providing a command-line HTTP client with built-in [MPP](https://mpp.dev) payment support, wallet identity management, and a release signing tool. The top-level `tempo` launcher lives in the main tempo repo (`tempo/crates/ext/`).

**Supported Payment Protocols:**
- [Machine Payments Protocol (MPP)](https://mpp.dev) - Open protocol for HTTP-native machine-to-machine payments

### Workspace Structure

The root `Cargo.toml` is workspace-only (no package). All dependencies are declared as `[workspace.dependencies]` in the root and consumed via `dep.workspace = true` in each crate. All crates live under `crates/`:

#### `crates/tempo-common/` — package `tempo-common` (library)

Shared library used by `tempo-wallet` and `tempo-request`. Contains all core logic:
- `crates/tempo-common/src/lib.rs` - Module declarations
- `crates/tempo-common/src/cli.rs` - Shared CLI infrastructure (`GlobalArgs`, `dispatch::track_command`, `dispatch::track_result`, `run_main`)
- `crates/tempo-common/src/context.rs` - `Context` struct (Config, NetworkId, Keystore, Analytics, OutputFormat) and `ContextArgs`
- `crates/tempo-common/src/config.rs` - Configuration file handling
- `crates/tempo-common/src/error.rs` - `TempoError` enum (thiserror)
- `crates/tempo-common/src/exit_codes.rs` - Process exit codes
- `crates/tempo-common/src/output.rs` - `OutputFormat`, structured output helpers
- `crates/tempo-common/src/runtime.rs` - Tracing, color mode, error rendering
- `crates/tempo-common/src/network.rs` - Network definitions (`NetworkId`), explorer config, RPC
- `crates/tempo-common/src/analytics.rs` - Opt-out telemetry (PostHog)
- `crates/tempo-common/src/util.rs` - Shared utilities (formatting, terminal hyperlinks, sanitization)
- `crates/tempo-common/src/account/` - Wallet account types (balances, spending limits) and on-chain queries
- `crates/tempo-common/src/http/` - HTTP client, request planning, response parsing
- `crates/tempo-common/src/keys/` - Key storage (model, I/O), signer resolution, authorization
- `crates/tempo-common/src/payment/` - Payment protocol implementations
  - `dispatch.rs` - Payment dispatch (route 402 flows to charge or session)
  - `charge.rs` - One-shot on-chain charge payment
  - `session/` - Session-based payment channels (channel.rs, close.rs, store.rs, streaming.rs, tx.rs)

#### `crates/tempo-wallet/` — package `tempo-wallet`, binary `tempo-wallet`

Wallet identity and custody extension, plus session/service management. Source organized by module directories:
- `crates/tempo-wallet/src/main.rs` - CLI entry point, calls `tempo_common::cli::run_main()`
- `crates/tempo-wallet/src/args.rs` - clap definitions (`Cli` with `#[command(flatten)] pub global: GlobalArgs`)
- `crates/tempo-wallet/src/app.rs` - Command dispatch: context building, command routing, analytics
- `crates/tempo-wallet/src/commands/` - Command implementations (all take `&Context` as first arg)
  - `login.rs` - Login command (passkey authentication flow)
  - `logout.rs` - Logout command
  - `whoami.rs` - Whoami command
  - `keys.rs` - Key listing, balance and spending limit queries
  - `wallets/` - Wallet management (create, list, fund/, keychain.rs)
  - `sessions/` - Session management (list, info, close, sync)
  - `services/` - Service directory listing and details
  - `sign.rs` - Sign MPP payment challenges
  - `completions.rs` - Shell completions
- `crates/tempo-wallet/tests/` - Integration tests (black-box CLI testing via assert_cmd)

#### `crates/tempo-sign/` — package `tempo-sign`, binary `tempo-sign`

Lightweight release manifest signing tool for authenticating build artifacts.
- `crates/tempo-sign/src/main.rs` - Signing tool source

**Packages:** `tempo-common`, `tempo-wallet`, `tempo-request`, `tempo-sign`

## Commands

```bash
make build              # Build debug binary
make release            # Build optimized release binary
make test               # Run all tests (uses mocks, no network required)
make check              # Run fmt check, clippy, tests, and doc
make fix                # Auto-fix formatting and clippy warnings
make install            # Install CLI binaries to ~/.local/bin
make uninstall          # Uninstall CLI binaries
make run ARGS="<url>"   # Run tempo-wallet with arguments
```

## Agent Suggestions

When the user explicitly says "ask the oracle" to check a value, run `tempo-request` against OpenRouter and explicitly tell the user which model was used in the response.

Example:
```bash
tempo-request -v -X POST --json '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"what is 1+1"}]}'  https://openrouter.mpp.tempo.xyz/v1/chat/completions | jq
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
- **Error handling**: Use `thiserror` for error types (`TempoError`), `anyhow` for propagation
- **Async runtime**: Tokio (minimal features: macros, rt-multi-thread, signal)
- **Serialization**: Serde with derive macros

### Imports

- Group imports: std → external crates → crate modules
- Use `use crate::` for internal module imports
- Use `use tempo_common::` for shared library imports

```rust
use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

use tempo_common::config::Config;
use tempo_common::error::TempoError;
```

### Error Handling Pattern

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum TempoError {
    #[error("failed to parse: {0}")]
    ParseError(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
```

### Module Organization

- Each module should have a clear single responsibility
- Use `mod.rs` for modules with submodules
- Shared logic goes in `crates/tempo-common/src/`
- All commands go in `crates/tempo-wallet/src/commands/`

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
- Flatten `GlobalArgs` from `tempo_common::cli` for shared flags
- Group related args with `help_heading`
- Support short aliases (`-v`) and long aliases (`--verbose`)

```rust
#[derive(Parser, Debug)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,
    
    #[command(subcommand)]
    pub command: Option<Commands>,
}
```

## Making Changes

### Before Starting Any Code Changes:
- [ ] Check existing patterns in similar files
- [ ] Identify affected tests

### Adding New Features

1. Add shared logic in `crates/tempo-common/src/`
2. Add CLI flags in the appropriate binary's `src/cli/args.rs`
3. Implement commands in the appropriate binary's `src/cli/commands/`
4. Add tests: unit tests in source files, integration tests in each crate's `tests/`

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

### Adding Dependencies

Add to `[workspace.dependencies]` in the root `Cargo.toml`, then reference with `dep.workspace = true` in the crate's `Cargo.toml`:

```toml
# Root Cargo.toml
[workspace.dependencies]
new-crate = "1.0"

# Crate Cargo.toml
[dependencies]
new-crate.workspace = true
```

## Environment Variables

| Variable | Description |
| -------- | ----------- |
| `TEMPO_RPC_URL` | Override RPC endpoint |
| `TEMPO_AUTH_URL` | Override auth server URL |
| `TEMPO_SERVICES_URL` | Override service directory API URL |
| `TEMPO_NO_TELEMETRY` | Disable telemetry |
| `TEMPO_PRIVATE_KEY` | Provide a private key directly for payment (bypasses wallet login and keychain; ephemeral) |

## Data Locations

**macOS:**
- Config: `~/Library/Application Support/tempo/wallet/config.toml`
- Wallet keys: `~/Library/Application Support/tempo/wallet/keys.toml`
- Private keys: macOS Keychain

**Linux:**
- Config: `~/.config/tempo/wallet/config.toml`
- Wallet keys: `~/.local/share/tempo/wallet/keys.toml`
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
