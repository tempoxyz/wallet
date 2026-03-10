# tempo-common

Shared library for Tempo CLI extension binaries. Provides common infrastructure used by `tempo-wallet` and `tempo-request`.

## Modules

| Module | Description |
|--------|-------------|
| `analytics` | Opt-out telemetry (PostHog) |
| `cli` | Shared CLI infrastructure (args, context, output, runner, runtime, tracking, formatting, terminal helpers) |
| `config` | Configuration file handling |
| `error` | Error types (`ConfigError`, `TempoError`) |
| `keys` | Key storage (model, I/O), signer resolution, authorization |
| `network` | Network definitions (`NetworkId`), explorer config, RPC |
| `payment` | Payment error classification and session management (persistence, channel queries, close, tx) |
| `security` | Security utilities (safe logging, sanitization, redaction) |

## Note

This is an internal crate — not published to crates.io. All user-facing behavior is exposed via the `tempo-wallet` and `tempo-request` CLI binaries.

## License

Dual-licensed under [Apache 2.0](../../LICENSE-APACHE) and [MIT](../../LICENSE-MIT).
