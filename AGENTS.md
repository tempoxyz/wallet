# AGENTS.md

## Repository Overview

This is a **Rust workspace** for `purl` - a curl-like CLI tool for making HTTP requests with automatic payment support.

**Supported Payment Protocols:**
- [x402](https://www.x402.org/) - HTTP 402 payment protocol for EVM and Solana
- [Web Payment Auth](https://datatracker.ietf.org/doc/draft-ietf-httpauth-payment/) - IETF standard for HTTP authentication-based payments

### Workspace Structure

| Crate | Description |
| ----- | ----------- |
| `lib/` | Core library (`purl-lib`) - payment providers, HTTP client, config, crypto, x402 protocol |
| `cli/` | CLI binary (`purl`) - command-line interface using clap |

## Commands

```bash
make build              # Build debug binary
make release            # Build optimized release binary
make test               # Run all tests (uses mocks, no network required)
make test-fast          # Run unit tests only (fastest)
make check              # Run fmt check, clippy, tests, and build
make fmt                # Auto-fix formatting and clippy warnings
make install            # Install CLI to ~/.cargo/bin
make run ARGS="<url>"   # Run CLI with arguments
```

## CRITICAL: Pre-Commit Requirements

### Before Every Commit, You MUST:

1. ✅ **Check**: `make check` - ZERO issues

## Code Style Guidelines

### Rust Conventions

- **Edition**: Rust 2021
- **Error handling**: Use `thiserror` for error types, `anyhow` for propagation
- **Async runtime**: Tokio with full features
- **Serialization**: Serde with derive macros

### Imports

- Group imports: std → external crates → internal modules
- Use `pub use` to re-export commonly used types from `lib.rs`

```rust
// lib.rs re-exports common types
pub use config::Config;
pub use error::{PurlError, Result};
pub use client::PurlClient;
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
- Re-export public types at module root

### Testing

- Integration tests go in `<crate>/tests/`
- Use the `common/mod.rs` pattern for shared test utilities
- Use `TestConfigBuilder` for setting up test configurations
- Test constants: `TEST_EVM_KEY`, `TEST_SOLANA_KEY` in `common/mod.rs`

**Writing Tests:**
- Avoid using `#[serial]` - prefer isolated temp directories with internal `_in_dir` helper functions
- Library tests should use `_in_dir` variants to avoid HOME env var conflicts

**Mock Mode for Network Tests:**
- Set `PURL_MOCK_NETWORK=1` environment variable to enable mock mode
- When enabled, the balance command returns fake data instead of making RPC calls
- Use `mock_test_command(&temp)` helper in integration tests for mock mode

```rust
use crate::common::{TestConfigBuilder, test_command};

#[test]
fn test_something() {
    let temp_dir = TestConfigBuilder::new()
        .with_default_evm()
        .build();
    
    let mut cmd = test_command(&temp_dir);
    cmd.arg("--help");
    cmd.assert().success();
}
```

### CLI Patterns (clap)

- Use derive macros for argument parsing
- Group related args with `help_heading`
- Support short aliases (`-v`) and long aliases (`--verbose`)
- Use environment variable fallbacks with `env = "PURL_*"`

```rust
#[derive(Parser, Debug)]
pub struct Cli {
    #[arg(short = 'v', long = "verbosity", action = clap::ArgAction::Count)]
    pub verbosity: u8,
    
    #[arg(long, env = "PURL_MAX_AMOUNT", help_heading = "Payment Options")]
    pub max_amount: Option<String>,
}
```

## Making Changes

### Before Starting Any Code Changes:
- [ ] Understand which crate(s) you're modifying (`lib/` vs `cli/`)
- [ ] Check existing patterns in similar files
- [ ] Identify affected tests

### Adding New Features

1. **Library changes** (`lib/`): Add core logic, expose via `lib.rs`
2. **CLI changes** (`cli/`): Add commands/flags in `cli.rs`, implement in appropriate `*_commands.rs`
3. **Add tests**: Both unit tests (in source files) and integration tests (in `tests/`)

## Dependencies

### Key External Crates

| Crate | Purpose |
| ----- | ------- |
| `clap` | CLI argument parsing |
| `alloy` / `alloy-signer` | EVM interactions and signing |
| `solana-sdk` / `solana-client` | Solana interactions |
| `curl` | HTTP client backend |
| `serde` / `serde_json` / `toml` | Serialization |
| `tokio` | Async runtime |
| `eth-keystore` | Encrypted keystore format |

### Adding Dependencies

- Add workspace-level dependencies in root `Cargo.toml` under `[workspace.dependencies]`
- Reference from crate `Cargo.toml` as `{ workspace = true }`

```toml
# Root Cargo.toml
[workspace.dependencies]
new-crate = "1.0"

# lib/Cargo.toml or cli/Cargo.toml
[dependencies]
new-crate = { workspace = true }
```

## Environment Variables

For testing and development, these environment variables are used:

| Variable | Description |
| -------- | ----------- |
| `HOME` | User home directory (for config/keystore paths) |
| `XDG_CONFIG_HOME` | Linux config directory |
| `XDG_DATA_HOME` | Linux data directory |
| `XDG_CACHE_HOME` | Linux cache directory |
| `PURL_MAX_AMOUNT` | Default max payment amount |
| `PURL_NETWORK` | Default network filter |
| `PURL_CONFIRM` | Require payment confirmation |
| `PURL_KEYSTORE` | Path to keystore file |
| `PURL_PASSWORD` | Keystore password (for CI/testing) |

## Data Locations

**macOS:**
- Config: `~/Library/Application Support/purl/config.toml`
- Keystores: `~/Library/Application Support/purl/keystores/`

**Linux:**
- Config: `~/.config/purl/config.toml`
- Keystores: `~/.local/share/purl/keystores/`

## Documentation

- [Rust Book](https://doc.rust-lang.org/book/)
- [Alloy Documentation](https://alloy.rs/)
- [Solana Cookbook](https://solanacookbook.com/)
- [x402 Protocol Spec](https://www.x402.org/)
