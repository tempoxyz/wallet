# Contributing to Tempo Wallet

Thanks for your interest in contributing to Tempo Wallet! This guide covers everything you need to build, test, and submit changes.

## Table of Contents

- [Prerequisites](#prerequisites)
- [Build & Test](#build--test)
- [Pre-Commit Checklist](#pre-commit-checklist)
- [Linting](#linting)
- [Project Structure](#project-structure)
- [Adding a New Feature](#adding-a-new-feature)
- [Testing](#testing)
- [Writing Documentation](#writing-documentation)
- [Changelogs](#changelogs)
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
make e2e                # Run live tests against mpp-proxy (requires funded wallet)
make coverage           # Generate code coverage (requires cargo-llvm-cov)
make install            # Install to ~/.local/bin
make uninstall          # Uninstall CLI
make run ARGS="<url>"   # Run with arguments
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
├── tempo-wallet/        # Main wallet HTTP client with MPP payment support
│   ├── src/
│   │   ├── main.rs              # CLI entry point, module declarations, error rendering
│   │   ├── error.rs             # Error types (thiserror)
│   │   ├── config.rs            # Configuration file handling
│   │   ├── network.rs           # Network definitions (Tempo, Moderato), explorer URLs, RPC
│   │   ├── util.rs              # Shared utilities (formatting, terminal hyperlinks, sanitization)
│   │   ├── analytics.rs         # Opt-out telemetry (PostHog)
│   │   ├── account/             # Wallet account types (balances, spending limits, on-chain queries)
│   │   │   ├── mod.rs           # Types (TokenBalance, KeyInfo, SpendingLimitInfo) and display helpers
│   │   │   └── query.rs         # On-chain balance and spending-limit queries
│   │   ├── http/                # HTTP client, request building, response parsing
│   │   │   ├── client.rs        # HttpClient, HttpRequestPlan, retry logic
│   │   │   └── response.rs      # HttpResponse type
│   │   ├── cli/                 # Argument parsing (clap) and command dispatch
│   │   │   ├── args.rs          # CLI argument definitions (Cli, QueryArgs, Commands)
│   │   │   ├── run.rs           # Application lifecycle: tracing, color, context, dispatch, analytics
│   │   │   ├── context.rs       # Context struct (shared app state threaded to all commands)
│   │   │   ├── output.rs        # OutputFormat, OutputOptions
│   │   │   ├── exit_codes.rs    # Process exit codes
│   │   │   └── commands/        # Command implementations (all take &Context)
│   │   │       ├── query/       # Query command (request → 402 → payment → response)
│   │   │       │   ├── mod.rs       # Main query flow
│   │   │       │   ├── context.rs   # HTTP client and output options building
│   │   │       │   ├── input.rs     # Header and body parsing helpers
│   │   │       │   ├── challenge.rs # 402 challenge parsing and wallet checks
│   │   │       │   ├── receipt.rs   # Payment receipt display and response output
│   │   │       │   ├── streaming.rs # SSE and streaming response handling
│   │   │       │   └── analytics.rs # Query-specific analytics tracking
│   │   │       ├── login/       # Login command and passkey authentication flow
│   │   │       ├── sessions/    # Session management (list, info, close, recover, sync)
│   │   │       ├── wallets/     # Wallet management (create, list, fund/, keychain.rs)
│   │   │       ├── logout.rs    # Logout command
│   │   │       ├── whoami.rs    # Whoami command
│   │   │       ├── keys.rs      # Key listing with balance and spending limit queries
│   │   │       ├── services.rs  # Service directory listing and details
│   │   │       └── completions.rs # Shell completions
│   │   ├── keys/                # Key storage, signing, and authorization
│   │   │   ├── model.rs         # KeyEntry, Keystore, WalletType types
│   │   │   ├── io.rs            # File I/O for keys.toml (load, save, keys_path)
│   │   │   ├── signer.rs        # Signing mode resolution (direct EOA vs keychain)
│   │   │   └── authorization.rs # Key authorization decoding and signing
│   │   └── payment/             # Payment protocol logic (MPP - https://mpp.dev)
│   │       ├── dispatch.rs      # Payment dispatch (route 402 flows to charge or session)
│   │       ├── charge.rs        # One-shot on-chain charge payment
│   │       └── session/         # Session-based payment channels
│   │           ├── channel.rs   # Channel open/query operations
│   │           ├── close.rs     # Channel close and finalization
│   │           ├── store.rs     # SQLite session persistence
│   │           ├── streaming.rs # Per-token voucher streaming
│   │           └── tx.rs        # Transaction building
│   └── tests/                   # Integration tests (black-box CLI testing via assert_cmd)
├── tempo-cli/           # Launcher and extension manager
│   ├── src/
│   └── tests/
├── sign-release/        # Release manifest signing tool
│   ├── src/
│   └── tests/
examples/                # Runnable example scripts
.changelog/              # Changelog entries (see Changelogs section)
```

### Scope: CLI-Only

This repository is a Cargo workspace with three binary crates (`tempo-wallet`, `tempo-cli`, `sign-release`). Internal modules are crate-private and not a stable public API. Please do not depend on any crate as a library — all supported behavior is exposed via the CLI.

### Key Conventions

**Imports** — group as std → external crates → crate modules:

```rust
use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

use crate::config::Config;
```

**Error handling** — use `thiserror` for error types, `anyhow` for propagation.

**Modules** — each module has a single responsibility. CLI commands go in `crates/tempo-wallet/src/cli/commands/`. Use `mod.rs` for modules with submodules.

## Adding a New Feature

1. Add core logic in the appropriate module under `crates/tempo-wallet/src/`
2. Add CLI flags/commands in `crates/tempo-wallet/src/cli/args.rs`, implement in `crates/tempo-wallet/src/cli/`
3. Add tests: unit tests in source files, integration tests in the relevant crate's `tests/` directory
4. Add a changelog entry (see [Changelogs](#changelogs))
5. Run `make check` — zero warnings required

## Testing

- **Unit tests** live in source files (`#[cfg(test)] mod tests`)
- **Integration tests** in each crate's `tests/` directory use `assert_cmd` for black-box CLI testing
- Use `TestConfigBuilder` and `test_command()` helpers to set up test configurations
- **Live tests:** `make e2e` runs tests against a live mpp-proxy (requires a funded wallet)
- **Coverage:** `make coverage` generates an lcov report (requires `cargo-llvm-cov` and `llvm-tools-preview`)

> **Note:** Tests use an in-memory keychain backend automatically (`InMemoryKeychain` via `#[cfg(test)]`), so they never touch the real OS keychain.

## Writing Documentation

Keep documentation in sync with the CLI. After changing flags, commands, or behavior:

1. Run `cargo run -- --help` (and `cargo run -- <subcommand> --help`) to verify help text is accurate
2. Update `README.md` if user-facing behavior changed (usage and examples only; repository-specific contributor guidance belongs here in CONTRIBUTING)
3. Check that `AGENTS.md` still reflects the current module layout and conventions

## Changelogs

This project uses [changelogs-rs](https://github.com/wevm/changelogs-rs) to manage changelog entries. Each PR that changes user-facing behavior should include a changelog entry.

### Adding an entry

Create a markdown file in `.changelog/` with a descriptive name and version-bump frontmatter:

```markdown
---
tempo-wallet: patch
---

Fixed a bug where session close would hang on timeout.
```

Valid bump levels: `major`, `minor`, `patch`.

### Configuration

Changelog settings live in `.changelog/config.toml`. The project uses a single root `CHANGELOG.md` (not per-crate). The [wevm/changelogs-rs](https://github.com/wevm/changelogs-rs) GitHub Action consumes these entries on release.

## Environment Variables

| Variable | Description |
|----------|-------------|
| `TEMPO_RPC_URL` | Override RPC endpoint |
| `TEMPO_AUTH_URL` | Override auth server URL |
| `TEMPO_NO_TELEMETRY` | Disable telemetry |
| `RUST_LOG` | Override tracing filter (e.g., `debug`, `info`) |
| `NO_COLOR` | Disable colored output (also disabled when stdout is not a terminal) |
| `TEMPO_PRIVATE_KEY` | *(hidden)* Provide a private key directly for payment — bypasses wallet login and keychain |
| `TEMPO_TEST_EVENTS` | *(internal)* Test hook — path to a file where analytics events are appended for assertion |
