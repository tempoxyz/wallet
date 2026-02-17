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
├── main.rs          # CLI entry point and request orchestration
├── error.rs         # Error types (thiserror)
├── cli/             # Argument parsing (clap) and command implementations
├── config/          # Configuration file handling
├── http/            # HTTP client and request building
├── network/         # Network definitions (Tempo, Moderato) and RPC
├── payment/         # Payment protocol logic (MPP - https://mpp.sh)
├── wallet/          # Wallet management, signing, and auth server
├── util/            # Shared utilities (atomic writes, constants)
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

**Modules** — each module has a single responsibility. CLI commands go in `cli/commands/` with `*_commands.rs` naming. Use `mod.rs` for modules with submodules.

**Testing** — unit tests live in source files (`#[cfg(test)] mod tests`). Integration tests in `tests/` use `assert_cmd` for black-box CLI testing. Use `TestConfigBuilder` and `test_command()` helpers.

**Mock mode** — set `PRESTO_MOCK_NETWORK=1` to enable mock mode for tests that would otherwise need network access.

## Adding a New Feature

1. Add core logic in the appropriate module under `src/`
2. Add CLI flags/commands in `src/cli/args.rs`, implement in `src/cli/commands/`
3. Add tests: unit tests in source files, integration tests in `tests/`
4. Update [SPEC.md](SPEC.md) if the change affects error messages, exit codes, or user-facing behavior
5. Run `make check` — zero warnings required

## Specification

[SPEC.md](SPEC.md) defines expected CLI behaviors — error message formats, exit codes, and user-facing messages. Changes that affect user-facing output should conform to the spec, or update it.

## Environment Variables

These are used for testing and development:

| Variable | Description |
|----------|-------------|
| `PRESTO_MOCK_NETWORK` | Enable mock mode for network calls in tests |
| `PRESTO_MOCK_PAYMENT` | Enable mock mode for payment flows in tests |
| `PRESTO_DEBUG` | Enable debug logging in the auth server |
| `PRESTO_NO_TELEMETRY` | Disable telemetry |
| `RUST_LOG` | Override tracing filter (e.g., `debug`, `info`) |
