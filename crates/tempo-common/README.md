# tempo-common

Shared library for Tempo CLI extension binaries. Provides common infrastructure used by `tempo-wallet` and `tempo-mpp`.

## Modules

| Module | Description |
|--------|-------------|
| `account` | Wallet account types (balances, spending limits) and on-chain queries |
| `analytics` | Opt-out telemetry (PostHog) |
| `cli` | Shared CLI infrastructure (`GlobalArgs`, dispatch tracking, `run_main`) |
| `config` | Configuration file handling |
| `context` | `Context` struct — shared app state threaded to all commands |
| `error` | `TempoError` enum (thiserror) |
| `exit_codes` | Process exit codes |
| `http` | HTTP client, request planning, response parsing |
| `keys` | Key storage (model, I/O), signer resolution, authorization |
| `network` | Network definitions (`NetworkId`), explorer config, RPC |
| `output` | `OutputFormat` and structured output helpers |
| `payment` | Payment protocol implementations (charge + session) |
| `runtime` | Tracing, color mode, error rendering |
| `util` | Shared utilities (formatting, terminal hyperlinks, sanitization) |

## Note

This is an internal crate — not published to crates.io. All user-facing behavior is exposed via the `tempo-wallet` and `tempo-mpp` CLI binaries.

## License

Dual-licensed under [Apache 2.0](../../LICENSE-APACHE) and [MIT](../../LICENSE-MIT).
