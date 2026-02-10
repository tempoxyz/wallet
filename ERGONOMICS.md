# tempoctl Ergonomics Feedback

**Version tested:** v0.1.0  
**Date:** 2026-02-10  
**Platform:** macOS arm64  
**State:** Tested both logged-out and logged-in (tempo-moderato testnet), made real paid requests against OpenAI, Anthropic, Firecrawl, Exa, Twitter, Storage, and RPC services.

---

## Overall Impression

tempoctl is well-structured with a clear mental model: it's "curl but it pays for you." The command surface area is right-sized, the help text is good, and features like `--dry-run`, `--confirm`, service aliases, and the `services info` command are genuinely useful. The actual payment flow works seamlessly for OpenAI and Firecrawl. Below is a catalog of rough edges found during hands-on exploration.

---

## Critical / P0 Bugs

### 1. `-vvv` crashes with exit code 101 (telemetry-related)

Running `tempoctl -vvv query <url>` exits with code 101 (Rust panic). The crash is caused by the PostHog telemetry subsystem -- setting `TEMPOCTL_NO_TELEMETRY=1` fixes it. `-v` and `-vv` work fine.

### 2. Services charge you even when they can't fulfill the request

Multiple services accept payment then fail:
- **Exa**: `"API key not configured for partner: exa"` -- payment taken ($0.01), service non-functional.
- **Twitter**: Same error -- `"API key not configured for partner: twitter"` -- payment taken ($0.01).
- **Anthropic** (without `anthropic-version` header): Payment taken ($0.00135), then `"anthropic-version: header is required"` error returned.

This is the most damaging UX issue. Users lose money on requests that were never going to work. The tool should either:
- Validate required headers before paying (for known services)
- Refuse to pay if the upstream service is not configured
- Refund failed requests

Exit code is 0 for all of these failures.

### 3. Single-letter command aliases include destructive `l` = `login`

Every major command has a single-letter alias: `q` (query), `l` (login), `n` (networks), `c` (config), `b` (balance), `v` (version), `w` (wallet), `k` (keys). This was discovered by accidentally running `tempoctl l`, which triggered a full OAuth login flow, opened the browser, and connected a wallet. A typo or tab-completion accident can connect a wallet unintentionally. `l` for login is especially dangerous.

### 4. `--json-output` / `--jo` silently ignored on most subcommands

`--json-output` has no effect on `balance`, `whoami`, `services list`, `networks list`, or `keys list`. The flag is accepted without error but produces plain text. Only `query` respects it.

### 5. `--output-format` placement is broken and inconsistent

- `tempoctl services --output-format json` (bare, no subcommand) → JSON output ✓
- `tempoctl services list --output-format json` → error: "unexpected argument"
- `tempoctl services --output-format json list` → error: "cannot be used with subcommand"
- `tempoctl services info openai --output-format json` → error: "unexpected argument"
- `tempoctl networks list --output-format json` → JSON ✓ (but `services list` doesn't!)
- `tempoctl networks info tempo --output-format json` → JSON ✓
- `tempoctl services --output-format yaml` → still outputs plain text table (yaml unsupported here)

The behavior is different between `services`, `networks`, and `keys`. Users will trip over this constantly.

---

## Bugs

### 6. Error messages reference non-existent `init` command

`tempoctl config get evm.address` outputs:
```
Run 'tempoctl init' to configure.
```
But `tempoctl init` does not exist. The correct command is `tempoctl login`.

### 7. `inspect` is broken on all payment endpoints

`tempoctl inspect` returns `Error: No payment required` for every URL tested, including actual payment endpoints like `https://openai.payments.tempo.xyz/v1/chat/completions`. It appears to only send a GET request and check for HTTP 402, but payment endpoints are POST-only and won't return 402 to a GET. The command needs to support specifying HTTP method, or it should use the service directory to show payment info without making a request.

### 8. `--dry-run` exits with error code 1 and prints "Error: Dry run completed"

```
[DRY RUN] Web Payment would be made:
...
Error: Dry run completed
```
Exit code 1 and the word "Error" for a successful operation. This breaks scripts that check exit codes.

### 9. `-d @-` and `-d @file` don't read from stdin/file

`-d @-` sends the literal string `@-` as the POST body instead of reading from stdin. `-d @/path/to/file` sends `@/path/to/file` as the POST body. curl supports both of these patterns. This is a fundamental gap for scripting.

### 10. `-o -` creates a file literally named `-`

`tempoctl query -o - <url>` creates a file named `-` in the current directory instead of writing to stdout. curl treats `-` as stdout.

### 11. HTTP error status codes don't propagate to exit codes

`tempoctl query https://httpbin.org/status/404` and `/status/500` both exit with code 0. For scripting, non-zero exit on 4xx/5xx is expected. There's no `--fail` flag equivalent.

### 12. Service upstream failures exit with code 0

When a service takes payment then fails (`"Upstream request failed after payment"`), the exit code is 0. The tool reports success for a failed request.

### 13. `config` shows "No payment methods configured" even when wallet is connected

After login, `tempoctl config` still says:
```
No payment methods configured.
Run 'tempoctl login' to configure payment methods.
```
Config and wallet state are stored separately (`config.toml` vs `wallet.toml`) and `config` doesn't reflect wallet state.

### 14. `balance` shows warnings for all networks including unreachable ones

Without `-n` filter, `balance` shows 8 warnings for failed balance queries on networks you're not using:
```
Warning: Failed to get pathUSD balance on tempo: HTTP error 401...
Warning: Failed to get pathUSD balance on tempo-localnet: error sending request...
```
It should only show balances for networks you have configured.

### 15. Multiple `-d` flags are rejected

`tempoctl query -d 'a=1' -d 'b=2'` fails with "cannot be used multiple times." curl concatenates multiple `-d` values with `&`. This breaks common curl-to-tempoctl migration patterns.

### 16. `-d` sends POST to GET-only endpoint without warning

`tempoctl query -d 'test' https://httpbin.org/get` silently sends a POST (implied by `-d`) and gets a 405. No warning that `-d` changed the method from GET to POST.

---

## Inconsistencies

### 17. `power-shell` vs `powershell`

`tempoctl completions` requires `power-shell` with a hyphen. Every other CLI tool uses `powershell`.

### 18. `-v` has two different meanings

- Global: `-v` is progressive verbosity (`-v`, `-vv`, `-vvv`)
- `query -i`: include headers in output
- `query -I`: headers only

curl users expect `-v` on a request to show headers. tempoctl requires `-i` for that. The skill doc says `--verbose` but the CLI says `--verbosity`.

### 19. `--json-output` vs `--output-format` overlap

Two flags that serve similar purposes with unclear distinction. `--json-output` (global) sometimes works, sometimes doesn't. `--output-format` (subcommand) sometimes exists, sometimes doesn't. No consistent pattern.

### 20. Spending limit warning text is ambiguous

```
Key spending limit (99.897807 pathUSD) is lower than wallet balance
```
This is not an error, just informational. But it reads like a warning that something is wrong. Better: show it once at login, or suppress it after the first time, or make it opt-in.

### 21. `--no-swap` has no visible documentation on what "swap" means

What tokens get swapped? What exchange rate? Is there slippage? The flag name implies token swaps but there's no explanation of the mechanism.

### 22. No default User-Agent

`tempoctl query` sends no User-Agent header (httpbin shows `null`). Most HTTP clients identify themselves. This could cause issues with APIs that reject requests without a User-Agent.

---

## Missing Features

### 23. No `config set` command

`config get` exists but `config set` does not. The only way to configure is `login` or manual TOML editing.

### 24. No transaction history

No `tempoctl history` or `tempoctl transactions` to see what you've paid for. For a payment tool, this is essential for auditing.

### 25. No spending summary

No way to see cumulative spend per service, per day, or per session. The spending limit per key is visible but there's no dashboard.

### 26. No `--fail` flag

No way to get non-zero exit codes on HTTP errors, unlike curl's `--fail`.

### 27. No service-name shorthand for `query`

Given the rich service registry, something like `tempoctl query openai /v1/chat/completions --json '{...}'` would reduce friction. Currently requires typing the full `https://openai.payments.tempo.xyz/v1/chat/completions` URL.

### 28. `wallet` subcommand is nearly empty

Only has `refresh`. No `wallet info`, `wallet export`, `wallet fund`. `balance` and `whoami` exist at top level but arguably belong here.

### 29. No man pages

Generates shell completions but not man pages.

### 30. No default `--max-amount` configuration

`TEMPOCTL_MAX_AMOUNT` env var exists but there's no way to persist it in config. Users who want a safety net must set the env var in their shell profile.

---

## UX Polish

### 31. Prices shown in dollars, `--max-amount` takes atomic units

`services info` shows `$0.05` but `--max-amount` takes `10000`. There's no documented conversion rate. The `--dry-run` output shows `Amount: 4688 (atomic units)` with no dollar equivalent. Even the balance output shows both: `2999999.918905 pathUSD (2999999918905 atomic units)` -- so the conversion is 1 pathUSD = 1,000,000 atomic units (6 decimals), but this is never stated.

### 32. Inconsistent exit codes

| Scenario | Exit Code |
|----------|-----------|
| balance (not logged in) | 1 |
| keys list (not logged in) | 3 |
| config get (not configured) | 3 |
| inspect (no payment) | 1 |
| query (DNS failure) | 4 |
| query (HTTP 404/500) | 0 |
| query (upstream failure after payment) | 0 |
| --dry-run (success) | 1 |
| -vvv crash | 101 |

No documented exit code contract.

### 33. PostHog telemetry fires 3 concurrent connections per request

Visible at `-vvv` (before the crash). Three simultaneous connections to `us.i.posthog.com`. `TEMPOCTL_NO_TELEMETRY` env var exists but is undocumented.

### 34. `whoami` defaults to testnet when not logged in

Shows `Network: tempo-moderato` with no explanation of why testnet is the default.

### 35. Redirects not followed by default

Unlike `wget` (which follows by default), `tempoctl query` requires explicit `-L` for redirects. This is curl's behavior, but the tool is described as "wget-like."

### 36. Wallet private key stored in plaintext

`wallet.toml` contains `private_key = "0x54b5..."` and `pending_key_authorization` in the clear. Also `config.toml` contains RPC credentials embedded in URLs (`https://user:pass@rpc.moderato.tempo.xyz`). File permissions are 644 on config.toml (world-readable) and 600 on wallet.toml (owner-only). The config.toml with RPC creds should also be 600.

### 37. `config --output-format json` shows `evm: null` even when wallet is active

Disconnect between config and wallet state makes it impossible to programmatically inspect the full state via a single command.

---

## What Works Well

- **Payment flow for working services** (OpenAI, Firecrawl, RPC) is seamless -- 402 → pay → response, invisible to the user.
- **`services info`** with alias lookup (`s3`, `claude`, `gpt`) is excellent.
- **`--max-amount`** correctly blocks overspending.
- **`-v` on payment requests** shows the full 402 challenge/response flow clearly.
- **`services --output-format json`** (bare command) returns rich structured data.
- **`-q` flag** correctly suppresses warnings to stderr.
- **`--json` flag on query** auto-sets Content-Type correctly.
- **Shell completions** work well.
- **Error messages** generally suggest the right fix (except for `init`).
- **Network filtering** (`-n tempo-moderato`) works consistently.

---

## Priority Summary

| Priority | # | Issue | Effort |
|----------|---|-------|--------|
| P0 | 1 | `-vvv` crash from telemetry | Small |
| P0 | 2 | Services charge on upstream failure (Exa, Twitter) | Medium |
| P0 | 3 | Single-letter `l` = `login` is destructive | Trivial |
| P0 | 4-5 | `--json-output` / `--output-format` broken | Medium |
| P0 | 8 | `--dry-run` exits 1 with "Error" message | Trivial |
| P0 | 9 | `-d @-` / `-d @file` don't work | Small |
| P0 | 36 | config.toml with RPC creds is world-readable (644) | Trivial |
| P1 | 6 | Error message references `init` | Trivial |
| P1 | 7 | `inspect` broken on POST endpoints | Medium |
| P1 | 10 | `-o -` creates file named `-` | Small |
| P1 | 11-12 | Exit codes don't reflect HTTP/service errors | Small |
| P1 | 13-14 | Config/balance state confusion | Small |
| P1 | 24 | Transaction history | Medium |
| P1 | 31 | Atomic units vs dollars confusion | Small |
| P2 | 15 | Multiple `-d` flags | Small |
| P2 | 17 | `power-shell` naming | Trivial |
| P2 | 22 | No default User-Agent | Trivial |
| P2 | 23 | `config set` | Small |
| P2 | 27 | Service-name shorthand for query | Medium |
| P2 | 30 | Default max-amount in config | Small |
| P3 | 25 | Spending summary | Large |
| P3 | 29 | Man pages | Small |
| P3 | 33 | Document telemetry opt-out | Trivial |
