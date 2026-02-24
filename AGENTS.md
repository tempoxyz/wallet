# AGENTS.md

## Repository Overview

This is `presto` - a pure binary crate providing a command-line HTTP client with built-in [MPP](https://mpp.sh) payment support.

**Supported Payment Protocols:**
- [Machine Payments Protocol (MPP)](https://mpp.sh) - Open protocol for HTTP-native machine-to-machine payments

### Crate Structure

Single binary crate with source organized by module directories:
- `src/main.rs` - CLI entry point and module declarations
- `src/cli/` - CLI argument parsing and all command implementations
  - `args.rs` - clap definitions (`Cli`, `QueryArgs`, `Commands`)
  - `query.rs` - Query command (request → 402 → payment → response)
  - `auth.rs` - Login, logout, whoami commands
  - `wallet.rs` - Wallet management (create/import/delete)
  - `session.rs` - Session list/close/recover commands
  - `output.rs` - Response display, `OutputOptions`
  - `exit_codes.rs` - Process exit codes
- `src/http.rs` - HTTP client, `RequestContext`, `RequestRuntime`
- `src/config.rs` - Configuration file handling
- `src/network.rs` - Network definitions, explorer config, RPC
- `src/payment/` - Payment protocol implementations (charge + session)
- `src/wallet/` - Wallet management and signing
  - `keychain.rs` - Platform-native secret storage (macOS Keychain)
- `src/analytics/` - Opt-out telemetry (PostHog)
- `src/util.rs` - Shared utilities (atomic writes, terminal hyperlinks)
- `src/error.rs` - Error types
- `tests/` - Integration tests (black-box CLI testing via assert_cmd)

**Package:** `presto` | **Binary:** `presto`

## Commands

```bash
make build              # Build debug binary
make release            # Build optimized release binary
make test               # Run all tests (uses mocks, no network required)
make check              # Run fmt check, clippy, tests, and build (linting handled in CI)
make fix                # Auto-fix formatting and clippy warnings
make install            # Install CLI to ~/.cargo/bin
make run ARGS="<url>"   # Run CLI with arguments
```

## Agent Suggestions

When the user explicitly says "ask the oracle" to check a value, run `presto` against OpenRouter and explicitly tell the user which model was used in the response.

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
2. Format as a clickable link: `[#123](https://github.com/tempoxyz/presto/pull/123)`
3. When creating multiple PRs, provide a summary table with all links

Example summary format:
```
| PR | Title | Link |
|----|-------|------|
| 1 | feat: add feature X | [#123](https://github.com/tempoxyz/presto/pull/123) |
| 2 | fix: resolve issue Y | [#124](https://github.com/tempoxyz/presto/pull/124) |
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
use crate::error::PrestoError;
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
- CLI commands go in `src/cli/` (e.g., `query.rs`, `auth.rs`, `session.rs`)

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
    #[arg(short = 'v', long = "verbose", action = clap::ArgAction::Count)]
    pub verbosity: u8,
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
| `mpp` | [Machine Payments Protocol](https://mpp.sh) SDK |

### Adding Dependencies

Add dependencies directly to `Cargo.toml`:

```toml
[dependencies]
new-crate = "1.0"
```

## Environment Variables

| Variable | Description |
| -------- | ----------- |
| `PRESTO_RPC_URL` | Override RPC endpoint |
| `PRESTO_AUTH_URL` | Override auth server URL |
| `PRESTO_NO_TELEMETRY` | Disable telemetry |
| `PRESTO_PRIVATE_KEY` | Provide a private key directly for payment (bypasses wallet login and keychain; ephemeral) |

## Data Locations

**macOS:**
- Config: `~/Library/Application Support/presto/config.toml`
- Wallet credentials: `~/Library/Application Support/presto/keys.toml`
- Private keys: macOS Keychain

**Linux:**
- Config: `~/.config/presto/config.toml`
- Wallet credentials: `~/.local/share/presto/keys.toml`
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
- `account_address` — On-chain account address
- `access_key_address` — Address of the access key (payment signing key)
- `access_key` — Access key stored inline; file is written with mode 0600
- `wallet_key_address` — Address of the wallet EOA key stored in the OS keychain
- `key_authorization` — On-chain authorization proof
- `provisioned_chain_ids` — Chains this key is provisioned on

**Network Resolution Priority:**
1. `PRESTO_RPC_URL` env var (overrides everything)
2. Typed overrides (`tempo_rpc`, `moderato_rpc`) take precedence
3. General `[rpc]` table as fallback
4. Default RPC if no override

**Built-in Networks:** `tempo` (chain 4217, mainnet), `tempo-moderato` (chain 42431, testnet)

**Built-in Tokens:** USDC (mainnet), pathUSD (testnet)

## Documentation

- [Rust Book](https://doc.rust-lang.org/book/)
- [Alloy Documentation](https://alloy.rs/)
