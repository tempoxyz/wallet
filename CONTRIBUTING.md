# Contributing to Tempo CLI

Thanks for your interest in contributing! This guide covers everything you need to build, test, and submit changes.

## Table of Contents

- [Prerequisites](#prerequisites)
- [Build & Test](#build--test)
- [Pre-Commit Checklist](#pre-commit-checklist)
- [Linting](#linting)
- [Project Structure](#project-structure)
- [Adding a New Feature](#adding-a-new-feature)
- [Testing](#testing)
- [Writing Documentation](#writing-documentation)
- [Environment Variables](#environment-variables)

## Prerequisites

- [Rust](https://rustup.rs/) (edition 2021)

```bash
git clone https://github.com/tempoxyz/wallet.git
cd wallet
make build
make test
```

## Build & Test

```bash
make build              # Debug build
make release            # Optimized release build
make test               # Run all tests (uses mocks, no network required)
make check              # fmt + clippy + test + doc
make fix                # Auto-fix formatting and clippy warnings
make coverage           # Generate code coverage (requires cargo-llvm-cov)
make install            # Install binaries to ~/.local/bin
make uninstall          # Uninstall binaries
make run ARGS="<url>"   # Run tempo-wallet with arguments
make clean              # cargo clean
```

## Pre-Commit Checklist

Before every commit, run:

```bash
make check
```

This runs `cargo fmt --check`, `cargo clippy -D warnings`, all tests, and doc generation. Everything must pass with **zero warnings**.

## Linting

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
crates/
├── tempo-common/        # Shared library for all extension binaries
│   └── src/
│       ├── lib.rs               # Module declarations
│       ├── cli.rs               # GlobalArgs, dispatch tracking, run_main()
│       ├── context.rs           # Context struct (shared app state for all commands)
│       ├── config.rs            # Configuration file handling
│       ├── error.rs             # TempoError enum (thiserror)
│       ├── exit_codes.rs        # Process exit codes
│       ├── output.rs            # OutputFormat, structured output helpers
│       ├── runtime.rs           # Tracing, color mode, error rendering
│       ├── network.rs           # Network definitions (Tempo, Moderato), explorer URLs, RPC
│       ├── analytics.rs         # Opt-out telemetry (PostHog)
│       ├── util.rs              # Shared utilities (formatting, terminal hyperlinks, sanitization)
│       ├── account/             # Wallet account types (balances, spending limits, on-chain queries)
│       ├── http/                # HTTP client, request building, response parsing
│       ├── keys/                # Key storage, signing, authorization
│       └── payment/             # Payment protocol logic (charge + session channels)
├── tempo-wallet/        # Wallet identity, custody, sessions, services, and signing
│   ├── src/
│   │   ├── main.rs              # CLI entry point (calls run_main)
│   │   └── commands/        # Command implementations
│   │           ├── login.rs         # Login (passkey auth flow)
│   │           ├── logout.rs        # Logout
│   │           ├── whoami.rs        # Whoami
│   │           ├── keys.rs          # Key listing with balance/spending limit queries
│   │           ├── wallets/         # Wallet management (create, list, fund)
│   │           ├── sessions/        # Session management (list, info, close, sync)
│   │           ├── services/        # Service directory listing and details
│   │           ├── sign.rs          # Sign MPP payment challenges
│   │           └── completions.rs   # Shell completions
│   └── tests/                   # Integration tests (assert_cmd)
└── tempo-sign/          # Release manifest signing tool
    └── src/main.rs
```

### Scope: CLI-Only

This repository is a Cargo workspace with binary crates and one internal shared library (`tempo-common`). Internal modules are crate-private and not a stable public API. Please do not depend on any crate as a library — all supported behavior is exposed via the CLI.

### Key Conventions

**Imports** — group as std → external crates → crate/tempo_common modules:

```rust
use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

use tempo_common::config::Config;
use tempo_common::error::TempoError;
```

**Error handling** — `TempoError` (thiserror) for typed errors, `anyhow` for propagation.

**Modules** — each module has a single responsibility. Shared logic goes in `tempo-common`. All commands go in `tempo-wallet/src/commands/`.

**Dependencies** — declared in `[workspace.dependencies]` in root `Cargo.toml`, referenced with `dep.workspace = true` in each crate.

## Adding a New Feature

1. Add shared logic in `crates/tempo-common/src/` if used by multiple binaries
2. Add CLI flags/commands in the appropriate binary's `src/cli/args.rs`
3. Implement commands in the appropriate binary's `src/cli/commands/`
4. Add tests: unit tests in source files, integration tests in the relevant crate's `tests/` directory
5. Run `make check` — zero warnings required

## Testing

- **Unit tests** live in source files (`#[cfg(test)] mod tests`)
- **Integration tests** in each crate's `tests/` directory use `assert_cmd` for black-box CLI testing
- Use `TestConfigBuilder` and `test_command()` helpers to set up test configurations
- **Coverage:** `make coverage` generates an lcov report (requires `cargo-llvm-cov` and `llvm-tools-preview`)

> **Note:** Tests use an in-memory keychain backend automatically (`InMemoryKeychain` via `#[cfg(test)]`), so they never touch the real OS keychain.

## Writing Documentation

Keep documentation in sync with the CLI. After changing flags, commands, or behavior:

1. Run `cargo run -p <crate> -- --help` (and subcommand `--help`) to verify help text is accurate
2. Update `README.md` if user-facing behavior changed
3. Check that `AGENTS.md` still reflects the current module layout and conventions

## Environment Variables

| Variable | Description |
|----------|-------------|
| `TEMPO_RPC_URL` | Override RPC endpoint |
| `TEMPO_AUTH_URL` | Override auth server URL |
| `TEMPO_SERVICES_URL` | Override service directory API URL |
| `TEMPO_NO_TELEMETRY` | Disable telemetry |
| `RUST_LOG` | Override tracing filter (e.g., `debug`, `info`) |
| `NO_COLOR` | Disable colored output (also disabled when stdout is not a terminal) |
| `TEMPO_PRIVATE_KEY` | *(hidden)* Provide a private key directly for payment — bypasses wallet login and keychain |
| `TEMPO_TEST_EVENTS` | *(internal)* Test hook — path to a file where analytics events are appended for assertion |
