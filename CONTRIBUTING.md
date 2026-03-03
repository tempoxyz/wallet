# Contributing to presto

Thanks for your interest in contributing to presto! This guide covers everything you need to build, test, and submit changes.

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
git clone https://github.com/tempoxyz/presto.git
cd presto
make build
make test
```

## Build & Test

```bash
make build              # Debug build
make release            # Optimized release build
make test               # Run all tests (uses mocks, no network required)
make check              # fmt + clippy + test + build
make fix                # Auto-fix formatting and clippy warnings
make e2e                # Run live tests against mpp-proxy (requires funded wallet)
make coverage           # Generate code coverage (requires cargo-llvm-cov)
make install            # Install to /usr/local/bin
make uninstall          # Uninstall CLI
make reinstall          # Rebuild and reinstall CLI
make run ARGS="<url>"   # Run with arguments
make clean              # cargo clean
```

## Pre-Commit Checklist

Before every commit, run:

```bash
make check
```

This runs `cargo fmt --check`, `cargo clippy -D warnings`, all tests, and a build. Everything must pass with **zero warnings**.

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
src/
├── main.rs              # CLI entry point and module declarations
├── error.rs             # Error types (thiserror)
├── http.rs              # HTTP client and request building
├── config.rs            # Configuration file handling
├── network.rs           # Network definitions (Tempo, Moderato) and RPC
├── util.rs              # Shared utilities (atomic writes, terminal hyperlinks)
├── cli/                 # Argument parsing (clap) and command implementations
│   ├── args.rs          # CLI argument definitions
│   ├── query.rs         # Query command (request → 402 → payment → response)
│   ├── auth.rs          # Login, logout, whoami
│   ├── keys.rs          # Key listing and spending limit queries
│   ├── local_wallet.rs  # Local wallet management (create/import/delete)
│   ├── fund.rs          # Wallet funding (faucet + bridge)
│   ├── relay.rs         # Relay bridge client for cross-chain funding
│   ├── services.rs      # Service directory listing and details
│   ├── session/         # Session management commands
│   ├── output.rs        # Response display
│   └── exit_codes.rs
├── payment/             # Payment protocol logic (MPP - https://mpp.dev)
│   ├── charge.rs        # One-shot on-chain charge payment
│   └── session/         # Session-based payment channels
├── wallet/              # Wallet management, signing, and credentials
│   ├── credentials/     # Credential storage and key management
│   ├── keychain.rs      # Platform-native secret storage (macOS Keychain)
│   ├── passkey.rs       # Browser-based passkey wallet flow
│   └── signer.rs        # Signing mode resolution
├── services/            # MPP service directory (registry fetching, data model)
└── analytics/           # Opt-out telemetry
tests/                   # Integration tests (black-box CLI testing via assert_cmd)
examples/                # Runnable example scripts
.changelog/              # Changelog entries (see Changelogs section)
```

### Scope: CLI-Only

This repository is a single binary crate. Internal modules are crate-private and not a stable public API. Please do not depend on presto as a library — all supported behavior is exposed via the CLI.

### Key Conventions

**Imports** — group as std → external crates → crate modules:

```rust
use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;

use crate::config::Config;
```

**Error handling** — use `thiserror` for error types, `anyhow` for propagation.

**Modules** — each module has a single responsibility. CLI commands go in `src/cli/`. Use `mod.rs` for modules with submodules.

## Adding a New Feature

1. Add core logic in the appropriate module under `src/`
2. Add CLI flags/commands in `src/cli/args.rs`, implement in `src/cli/`
3. Add tests: unit tests in source files, integration tests in `tests/`
4. Add a changelog entry (see [Changelogs](#changelogs))
5. Run `make check` — zero warnings required

## Testing

- **Unit tests** live in source files (`#[cfg(test)] mod tests`)
- **Integration tests** in `tests/` use `assert_cmd` for black-box CLI testing
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
presto: patch
---

Fixed a bug where session close would hang on timeout.
```

Valid bump levels: `major`, `minor`, `patch`.

### Configuration

Changelog settings live in `.changelog/config.toml`. The project uses a single root `CHANGELOG.md` (not per-crate). The [wevm/changelogs-rs](https://github.com/wevm/changelogs-rs) GitHub Action consumes these entries on release.

## Environment Variables

| Variable | Description |
|----------|-------------|
| `PRESTO_RPC_URL` | Override RPC endpoint |
| `PRESTO_AUTH_URL` | Override auth server URL |
| `PRESTO_NO_TELEMETRY` | Disable telemetry |
| `RUST_LOG` | Override tracing filter (e.g., `debug`, `info`) |
| `NO_COLOR` | Disable colored output (also disabled when stdout is not a terminal) |
| `PRESTO_PRIVATE_KEY` | *(hidden)* Provide a private key directly for payment — bypasses wallet login and keychain |
| `PRESTO_TEST_EVENTS` | *(internal)* Test hook — path to a file where analytics events are appended for assertion |
