# Contributing to presto

Thanks for your interest in contributing to presto! This guide covers everything you need to build, test, and submit changes.

## Getting Started

### Prerequisites

- [Rust](https://rustup.rs/) (edition 2021)
- [Node.js](https://nodejs.org/) (for linting only)

### Setup

```bash
git clone https://github.com/tempoxyz/presto.git
cd presto
make build
make test
```

## Development Workflow

### Build & Test

```bash
make build          # Debug build
make release        # Optimized release build
make test           # Run all tests (uses mocks, no network required)
make check          # fmt + clippy + test + build
make fix            # Auto-fix formatting and clippy warnings
make install        # Install to ~/.cargo/bin
make run ARGS="<url>"  # Run with arguments
```

### Pre-Commit Checklist

Before every commit, run:

```bash
make check
```

This runs `cargo fmt --check`, `cargo clippy`, all tests, and a build. All must pass with zero warnings.

### Linting

This project uses [Tempo lints](https://github.com/tempoxyz/lints) for additional code quality checks beyond clippy:

```bash
npm install         # Install lint tooling (first time only)
npm run lint        # Run lints
```

> **Note:** Use `npm` (not `pnpm`) — the `@tempoxyz/lints` package uses build scripts that pnpm v10 blocks.

To suppress a lint for a specific line:

```rust
// ast-grep-ignore: no-unwrap-in-lib
let value = something.unwrap();
```

## Project Structure

```
src/
├── main.rs          # CLI entry point and module declarations
├── error.rs         # Error types (thiserror)
├── http.rs          # HTTP client and request building
├── config.rs        # Configuration file handling
├── network.rs       # Network definitions (Tempo, Moderato) and RPC
├── util.rs          # Shared utilities (atomic writes, terminal hyperlinks)
├── cli/             # Argument parsing (clap) and command implementations
│   ├── args.rs      # CLI argument definitions
│   ├── query.rs     # Query command (request → 402 → payment → response)
│   ├── auth.rs      # Login, logout, whoami
│   ├── wallet.rs    # Wallet management (create/import/delete)
│   ├── session.rs   # Session list/close/recover
│   ├── output.rs    # Response display
│   └── exit_codes.rs
├── payment/         # Payment protocol logic (MPP - https://mpp.sh)
├── wallet/          # Wallet management, signing, and auth server
│   └── keychain.rs  # Platform-native secret storage (macOS Keychain)
└── analytics/       # Opt-out telemetry
tests/               # Integration tests (black-box CLI testing via assert_cmd)
examples/            # Runnable example scripts
```

### Key Conventions

**Imports** — group as std → external crates → crate modules:

```rust
use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

use crate::config::Config;
```

**Error handling** — use `thiserror` for error types, `anyhow` for propagation.

**Modules** — each module has a single responsibility. CLI commands go in `src/cli/` (e.g., `query.rs`, `auth.rs`, `session.rs`). Use `mod.rs` for modules with submodules.

**Testing** — unit tests live in source files (`#[cfg(test)] mod tests`). Integration tests in `tests/` use `assert_cmd` for black-box CLI testing. Use `TestConfigBuilder` and `test_command()` helpers.

## Adding a New Feature

1. Add core logic in the appropriate module under `src/`
2. Add CLI flags/commands in `src/cli/args.rs`, implement in `src/cli/`
3. Add tests: unit tests in source files, integration tests in `tests/`
4. Run `make check` — zero warnings required

## Environment Variables

| Variable | Description |
|----------|-------------|
| `PRESTO_RPC_URL` | Override RPC endpoint |
| `PRESTO_AUTH_URL` | Override auth server URL |
| `PRESTO_NO_TELEMETRY` | Disable telemetry |
| `RUST_LOG` | Override tracing filter (e.g., `debug`, `info`) |

> **Note:** Unit tests use an in-memory keychain backend automatically (`InMemoryKeychain` via `#[cfg(test)]`), so they never touch the real OS keychain.
