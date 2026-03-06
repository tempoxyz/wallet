# Tempo CLI

The `tempo` command: routes to extensions, manages installs, and provides a unified entry point for the Tempo ecosystem.

## Scope

1. Provide `tempo <extension>` UX.
2. Route to extension binaries (`tempo-wallet`, future `tempo-<x>`).
3. Preserve legacy core node command surface via fallback to `tempo-core`.
4. Keep extension discovery deterministic for automation and agents.

## Command Contract (MVP)

Given `tempo <extension> [args...]`:

1. Handle builtins: `help`, `version`.
2. Execute `tempo-<extension>` if present.
3. For extensions, if missing locally, attempt auto-install from `https://cli.tempo.xyz/extensions/tempo-<extension>/manifest.json`.
4. For core subcommands (`node`, `init`, `db`, `core`, `consensus`, `init-from-binary-dump`), if missing locally, attempt install via `tempoup` and rewire the installed node payload to `tempo-core`.
5. If none match, return install guidance.

Core install guidance is always: `Run: tempoup`.

Given `tempo` with no args:

1. Execute `tempo-core` with no args if installed.
2. Otherwise print help.

## Binary Lookup Order

1. Same directory as `tempo` binary.
2. `PATH`.

## Build

```bash
cargo build
```

## Run Local Examples

```bash
cargo run -- --help
cargo run -- wallet https://api.example.com
```

## Installer Contract (MVP)

`tempo add/update/remove` installs and manages extension binaries in `~/.local/bin` (override with `TEMPO_HOME` to install to `$TEMPO_HOME/bin` instead).

```bash
# install/update using signed manifest
cargo run -- add wallet
cargo run -- update wallet

# install/update with explicit manifest and key
cargo run -- add wallet --release-manifest https://cli.tempo.xyz/extensions/tempo-wallet/manifest.json --release-public-key <base64-ed25519-pubkey>

# remove
cargo run -- remove wallet
```

Notes:

1. Each extension installs a single binary (`tempo-<extension>`).
2. Installs are additive and idempotent.
3. `--release-manifest` supports `https://` URL, `file://` URL, and local path sources.
4. Manifest installs enforce both SHA256 and Ed25519 signature verification before any copy into the install directory.
5. `--release-public-key` is required for manual `tempo add/update --release-manifest` installs; runtime auto-install uses the baked-in key.
6. `--dry-run` is available on install/update/remove.
7. Runtime auto-install uses `https://cli.tempo.xyz/extensions/tempo-<extension>/manifest.json`.
8. Runtime auto-install uses the Ed25519 release key baked into the binary.
9. Core auto-install fallback requires `tempoup` to be available on `PATH`.
10. Set `TEMPO_DEBUG=1` to print routing and auto-install debug decisions to stderr.
11. Manual manifest installs via `tempo add/update --release-manifest` reject insecure `http://` URLs.
12. Legacy `tempoup` state migration is not performed inside `tempo add/update/remove`; handle migration in install/bootstrap tooling.

## Install

```bash
./install              # build tempo CLI, install wallet from remote manifest
./install --uninstall  # remove all installed binaries
```

## Scripts

1. `scripts/local-e2e-test.sh` — End-to-end tests using file:// manifests (no network required).
2. `scripts/remote-e2e-test.sh` — End-to-end tests against the production manifest at `https://cli.tempo.xyz` (requires network and a published release).

## Internals

- Core subcommand routing source-of-truth: `CORE_SUBCOMMANDS` constant in `src/launcher.rs`.
- Release manifests carry SHA256 checksums and Ed25519 signatures; `tempo` verifies both before installing.
- Core auto-install fallback invokes `tempoup` on `PATH` and rewires the result to `tempo-core`.
