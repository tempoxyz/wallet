# PLAN.md — Unambiguous Feedback Items

## Help & CLI Design

- [x] `--help` should show both short and long forms for all args (e.g., `-v, --verbosity`)
- [x] `version` subcommand → remove in favor of `-V` / `--version` (already exists as a flag, subcommand is redundant)
- [x] Add command categories/groups in `--help` output (e.g., "Core", "Wallet", "Config")

## Remove or Consolidate

- [x] Remove `tempoctl config` subcommand — unnecessary for a simple tool
- [x] Merge `whoami` and `keys` into a single command (whoami already shows access keys)

## UX

- [x] `tempoctl q` without credentials → print helpful message to stderr explaining how to log in, then exit
- [x] `tempoctl completions` without a shell argument → show list of supported shells instead of erroring

## Config / Networks

- [x] `TEMPOCTL_NETWORK` example text should show `tempo, tempo-moderato` not `base, base-sepolia`

## Bugs

- [x] Key spending limit < wallet balance display doesn't make sense — fix the display/logic

## From Ergonomics Review

### P0 — Trivial/Small

- [x] **#1**: `-vvv` crashes with exit code 101 (telemetry panic) — fix or catch the panic
- [x] **#3**: Remove single-letter command aliases (especially `l` = `login`) — too dangerous
- [x] **#8**: `--dry-run` exits 1 with "Error: Dry run completed" — exit 0, no "Error" prefix
- [x] **#9**: `-d @-` and `-d @file` don't read from stdin/file — implement curl-compatible `@` syntax
- [x] **#36**: `config.toml` with RPC creds is world-readable (644) — set to 600 on write
- [x] **#6**: Error message references non-existent `init` command — change to `login`
- [x] **#10**: `-o -` creates a file named `-` — treat `-` as stdout
- [x] **#17**: `power-shell` → `powershell` in completions
- [x] **#22**: No default User-Agent header — send `tempoctl/<version>`

### P0 — Medium

- [x] **#4-5**: `--json-output` / `--output-format` broken and inconsistent — unify into one consistent flag across all subcommands

### P1

- [ ] **#11-12**: HTTP error status codes and upstream failures exit 0 — add `--fail` flag or always exit non-zero on 4xx/5xx
- [ ] **#31**: `--max-amount` takes atomic units but prices shown in dollars — accept dollar amounts (e.g., `--max-amount 0.05`), show dollar equivalents in `--dry-run`. Make sure help docs are clean.

### P2

- [ ] **#15**: Multiple `-d` flags rejected — concatenate with `&` like curl
- [ ] **#30**: No way to persist default `--max-amount` in config
