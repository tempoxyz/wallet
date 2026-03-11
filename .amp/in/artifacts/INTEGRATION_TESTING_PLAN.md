# Integration Testing Plan — Tempo CLI Workspace

## Overview

This document is the comprehensive integration test plan for `tempo-request` and `tempo-wallet`. All tests **must** run without network access — every external dependency (HTTP servers, RPC endpoints, service directories) is mocked locally using axum.

---

## Part 1: Framework Problems & Solutions

### Problem 1: Duplicated Common Modules

The `tests/common/mod.rs` files in `tempo-request` and `tempo-wallet` are **95% identical** — only the binary name in `cargo_bin!()` differs. Shared helpers (`TestConfigBuilder`, `write_test_files`, `get_combined_output`, `seed_local_session`, `delete_sessions_db`) are copy-pasted between them.

**Solution:** Create a `crates/tempo-test-utils` library crate that both binary crates depend on as `[dev-dependencies]`. The binary name is passed as a parameter:

```rust
// crates/tempo-test-utils/src/lib.rs
pub fn test_command(binary: &str, temp_dir: &TempDir) -> Command {
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin(binary));
    cmd.env("HOME", temp_dir.path());
    cmd.env("TEMPO_NO_AUTO_LOGIN", "1");
    cmd.env("TEMPO_NO_AUTO_JSON", "1");
    cmd
}
```

Each crate's thin `common/mod.rs` becomes a one-liner re-export:

```rust
// crates/tempo-request/tests/common/mod.rs
pub use tempo_test_utils::*;
pub fn test_command(temp_dir: &tempfile::TempDir) -> std::process::Command {
    tempo_test_utils::test_command("tempo-request", temp_dir)
}
```

### Problem 2: MockServer Lives in query.rs (1 file, 268 lines)

`MockServer`, `MockRpcServer`, `mock_rpc_response` are all defined inline in `query.rs`. They can't be reused by `structured.rs`, `cli.rs`, or any `tempo-wallet` test. The wallet's `structured.rs` already duplicates a manual axum server setup for the services mock.

**Solution:** Move all mock servers into `tempo-test-utils`:

```
crates/tempo-test-utils/src/
├── lib.rs          // Re-exports
├── config.rs       // TestConfigBuilder, write_test_files
├── command.rs      // test_command, get_combined_output
├── session.rs      // seed_local_session, delete_sessions_db
├── mock_http.rs    // MockServer (generic HTTP, echo, SSE, payment)
├── mock_rpc.rs     // MockRpcServer, mock_rpc_response
├── mock_services.rs // MockServicesServer (services directory)
├── wallet.rs       // Test wallet constants, setup_live_test
└── assert.rs       // Assertion helpers (exit codes, structured output)
```

### Problem 3: Payment Challenge Boilerplate

The base64url challenge string is copy-pasted **14 times** across `query.rs`. Each payment test rebuilds the same `www_auth` format string and `keys_toml` block manually.

**Solution:** Pre-built constants and a builder in `tempo-test-utils`:

```rust
// crates/tempo-test-utils/src/wallet.rs
pub const HARDHAT_PRIVATE_KEY: &str = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
pub const HARDHAT_ADDRESS: &str = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266";

/// Standard keys.toml for Moderato charge tests (Hardhat #0, Direct signing mode).
pub const MODERATO_DIRECT_KEYS_TOML: &str = r#"
[[keys]]
wallet_address = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
key_address = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
key = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
chain_id = 42431
"#;

/// Standard keys.toml for Keychain signing mode.
pub const MODERATO_KEYCHAIN_KEYS_TOML: &str = r#"
[[keys]]
wallet_address = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8"
chain_id = 42431
key_address = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
key = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
provisioned = true
"#;

// crates/tempo-test-utils/src/mock_http.rs

/// Base64url-encoded Moderato charge challenge (1 USDC to Hardhat #1).
pub const MODERATO_CHARGE_CHALLENGE: &str = "eyJhbW91bnQiOiIxMDAwMDAwIiwiY3VycmVuY3kiOiIweDIwYzAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAiLCJtZXRob2REZXRhaWxzIjp7ImNoYWluSWQiOjQyNDMxfSwicmVjaXBpZW50IjoiMHg3MDk5Nzk3MEM1MTgxMmRjM0EwMTBDN2QwMWI1MGUwZDE3ZGM3OUM4In0";

/// Build a WWW-Authenticate header for a charge challenge.
pub fn charge_www_authenticate(id: &str) -> String {
    format!(
        r#"Payment id="{id}", realm="mock", method="tempo", intent="charge", request="{MODERATO_CHARGE_CHALLENGE}""#
    )
}
```

### Problem 4: No Structured Output Assertion Helpers

Tests for JSON/TOON output repeat the same parse-and-assert pattern. The `run_structured`/`run_both` helpers exist in each `structured.rs` but aren't shared and only handle success cases.

**Solution:** Shared assertion helpers in `tempo-test-utils`:

```rust
// crates/tempo-test-utils/src/assert.rs
use serde_json::Value;

/// Assert clean stderr (empty) for structured output modes.
pub fn assert_clean_stderr(output: &Output) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.trim().is_empty(), "structured mode should not write to stderr: {stderr}");
}

/// Assert the process exited with a specific exit code.
pub fn assert_exit_code(output: &Output, expected: i32, context: &str) {
    assert_eq!(
        output.status.code(),
        Some(expected),
        "expected exit code {expected}: {context}"
    );
}

/// Parse stdout as JSON, panicking with context on failure.
pub fn parse_json_stdout(output: &Output) -> Value {
    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("invalid JSON stdout: {e}\n---\n{stdout}"))
}

/// Parse stdout as TOON, panicking with context on failure.
pub fn parse_toon_stdout(output: &Output) -> Value {
    let stdout = String::from_utf8_lossy(&output.stdout);
    toon_format::decode_default(stdout.trim())
        .unwrap_or_else(|e| panic!("invalid TOON stdout: {e}\n---\n{stdout}"))
}

/// Assert a structured error response has the expected code.
pub fn assert_structured_error(output: &Output, expected_code: &str) {
    let json = parse_json_stdout(output);
    assert_eq!(json["code"], expected_code, "wrong error code in: {json}");
    assert!(json["message"].is_string(), "missing message in: {json}");
}

/// Run a command in all three output formats and return (text, json, toon) outputs.
pub fn run_all_formats(
    temp: &TempDir,
    binary: &str,
    args: &[&str],
) -> (Output, Output, Value, Output, Value) {
    let text_out = test_command(binary, temp).args(args).output().unwrap();

    let mut json_args = vec!["-j"];
    json_args.extend_from_slice(args);
    let json_out = test_command(binary, temp).args(&json_args).output().unwrap();
    let json_val = parse_json_stdout(&json_out);

    let mut toon_args = vec!["-t"];
    toon_args.extend_from_slice(args);
    let toon_out = test_command(binary, temp).args(&toon_args).output().unwrap();
    let toon_val = parse_toon_stdout(&toon_out);

    (text_out, json_out, json_val, toon_out, toon_val)
}
```

### Problem 5: Payment Test Setup Is ~15 Lines Each

Every payment flow test requires: start MockRpcServer → build challenge → start MockServer::start_payment → build TestConfigBuilder with keys_toml + config_toml pointing to mock RPC. This is **15+ lines** of identical boilerplate per test.

**Solution:** A `PaymentTestHarness` builder that does everything:

```rust
// crates/tempo-test-utils/src/mock_http.rs

/// Complete harness for 402→payment→200 integration tests.
pub struct PaymentTestHarness {
    pub rpc: MockRpcServer,
    pub server: MockServer,
    pub temp: TempDir,
}

impl PaymentTestHarness {
    /// Standard Moderato charge flow with Direct signing mode.
    pub async fn charge() -> Self {
        Self::charge_with_id("test-charge").await
    }

    pub async fn charge_with_id(id: &str) -> Self {
        let rpc = MockRpcServer::start(42431).await;
        let www_auth = charge_www_authenticate(id);
        let server = MockServer::start_payment(&www_auth, "ok").await;
        let temp = TestConfigBuilder::new()
            .with_keys_toml(MODERATO_DIRECT_KEYS_TOML)
            .with_config_toml(format!("moderato_rpc = \"{}\"\n", rpc.base_url))
            .build();
        PaymentTestHarness { rpc, server, temp }
    }

    /// Charge flow with Keychain signing mode.
    pub async fn charge_keychain() -> Self {
        let rpc = MockRpcServer::start(42431).await;
        let www_auth = charge_www_authenticate("test-kc");
        let server = MockServer::start_payment(&www_auth, "ok").await;
        let temp = TestConfigBuilder::new()
            .with_keys_toml(MODERATO_KEYCHAIN_KEYS_TOML)
            .with_config_toml(format!("moderato_rpc = \"{}\"\n", rpc.base_url))
            .build();
        PaymentTestHarness { rpc, server, temp }
    }

    /// Charge flow with custom success body.
    pub async fn charge_with_body(body: &str) -> Self {
        let rpc = MockRpcServer::start(42431).await;
        let www_auth = charge_www_authenticate("test-charge");
        let server = MockServer::start_payment(&www_auth, body).await;
        let temp = TestConfigBuilder::new()
            .with_keys_toml(MODERATO_DIRECT_KEYS_TOML)
            .with_config_toml(format!("moderato_rpc = \"{}\"\n", rpc.base_url))
            .build();
        PaymentTestHarness { rpc, server, temp }
    }

    pub fn url(&self, path: &str) -> String {
        self.server.url(path)
    }
}
```

**Before (14 lines per test):**
```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_402_charge_flow() {
    let rpc = MockRpcServer::start(42431).await;
    let challenge_request = "eyJhbW91bnQiOiIxMDAwMDAwIiwiY3VycmVuY3kiOiIweDIwYzAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAiLCJtZXRob2REZXRhaWxzIjp7ImNoYWluSWQiOjQyNDMxfSwicmVjaXBpZW50IjoiMHg3MDk5Nzk3MEM1MTgxMmRjM0EwMTBDN2QwMWI1MGUwZDE3ZGM3OUM4In0";
    let www_auth = format!(
        r#"Payment id="test-charge", realm="mock", method="tempo", intent="charge", request="{challenge_request}""#
    );
    let server = MockServer::start_payment(&www_auth, "charge accepted").await;
    let temp = TestConfigBuilder::new()
        .with_keys_toml(r#"
[[keys]]
wallet_address = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
key_address = "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
key = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
chain_id = 42431
"#)
        .with_config_toml(format!("moderato_rpc = \"{}\"\n", rpc.base_url))
        .build();
    // ... actual test logic
}
```

**After (3 lines per test):**
```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_402_charge_flow() {
    let h = PaymentTestHarness::charge_with_body("charge accepted").await;
    // ... actual test logic using h.url("/api") and h.temp
}
```

### Problem 6: No Easy Way to Test a New Extension Binary

If someone adds `tempo-foo` as a new extension binary, they'd need to:
1. Copy the entire `common/mod.rs` (again)
2. Duplicate MockServer code (again)
3. Figure out the patterns by reading existing test files

**Solution:** With `tempo-test-utils`, a new extension's test setup is:

```toml
# crates/tempo-foo/Cargo.toml
[dev-dependencies]
tempo-test-utils = { path = "../tempo-test-utils" }
```

```rust
// crates/tempo-foo/tests/common/mod.rs
pub use tempo_test_utils::*;
pub fn test_command(temp_dir: &tempfile::TempDir) -> std::process::Command {
    tempo_test_utils::test_command("tempo-foo", temp_dir)
}
```

Then every test can immediately use `TestConfigBuilder`, `MockServer`, `PaymentTestHarness`, `assert_exit_code`, `run_all_formats`, etc. Zero boilerplate.

---

## Part 2: Proposed `tempo-test-utils` Crate Layout

```
crates/tempo-test-utils/
├── Cargo.toml
└── src/
    ├── lib.rs              // Re-exports all modules
    ├── config.rs           // TestConfigBuilder, write_test_files
    ├── command.rs          // test_command(), get_combined_output()
    ├── session.rs          // seed_local_session(), delete_sessions_db()
    ├── wallet.rs           // Test key constants, signing mode presets
    ├── assert.rs           // assert_exit_code, assert_clean_stderr, parse_json/toon,
    │                       // assert_structured_error, run_all_formats
    ├── mock_http.rs        // MockServer (generic, echo, SSE, payment, payment+receipt)
    ├── mock_rpc.rs         // MockRpcServer, mock_rpc_response
    ├── mock_services.rs    // MockServicesServer (service directory)
    └── harness.rs          // PaymentTestHarness (compose RPC + HTTP + config)
```

**Cargo.toml:**
```toml
[package]
name = "tempo-test-utils"
version = "0.1.0"
edition = "2021"
publish = false

[dependencies]
assert_cmd.workspace = true
axum.workspace = true
rusqlite.workspace = true
serde_json.workspace = true
tempfile.workspace = true
tokio.workspace = true
toon-format.workspace = true
```

---

## Part 3: Existing Coverage Inventory (88 tests)

### `tempo-request/tests/query.rs` — 68 tests ✅

**HTTP Basics (11):**
- `test_non_402_get_request` — GET 200
- `test_non_402_post_with_json` — POST with `--json`
- `test_include_headers_flag` — `-i` shows status line + headers
- `test_output_to_file` — `-o FILE`
- `test_server_error_500` — Non-zero exit on 500
- `test_connection_refused` — Connection error handling
- `test_custom_header` — `-H "X-Custom: val"`
- `test_post_data_flag` — `-d key=value`
- `test_post_data_from_file` — `-d @file`
- `test_multiple_data_flags` — Multiple `-d` flags
- `test_timeout_flag` — `--timeout`

**Verbosity/Quiet (3):**
- `test_quiet_suppresses_logs` — `-s`
- `test_verbose_shows_logs` — `-v`
- `test_retries_and_backoff_on_unreachable_host` — `--retries` + JSON error

**Output Formats (2):**
- `test_output_format_json` — `-j` passes through JSON body
- `test_toon_output_pretty_prints_json_response` — `-t` converts JSON body to TOON

**Redirects (1):**
- `test_no_redirect` — 301 without `-L`

**Dump Headers (1):**
- `test_dump_header_writes_file` — `-D FILE`

**402 Payment — Charge Flow (6):**
- `test_402_charge_flow` — Full 402→payment→200
- `test_402_charge_flow_keychain` — Keychain signing mode
- `test_402_payment_narration_verbose` — Verbose narration
- `test_402_paid_summary_default_and_quiet` — Paid summary + `-s` suppression
- `test_analytics_tx_hash_is_extracted_hex` — Receipt tx_hash extraction
- `test_402_without_valid_payment_header` — 402 missing WWW-Authenticate

**402 Edge Cases (6):**
- `test_402_unsupported_payment_method` — Non-tempo method
- `test_402_without_www_authenticate_header` — Missing header
- `test_402_without_www_authenticate_json_error` — JSON error schema
- `test_402_malformed_www_authenticate` — Invalid challenge format
- `test_402_empty_body_no_crash` — Empty body 402
- `test_server_error_json_output_schema` — 500 error schema

**Private Key (7):**
- `test_402_charge_flow_with_private_key_flag` — `--private-key` flag
- `test_402_charge_flow_with_private_key_env` — `TEMPO_PRIVATE_KEY` env
- `test_private_key_without_0x_prefix` — No `0x` prefix
- `test_private_key_invalid_hex_fails` — Invalid hex
- `test_private_key_wrong_length_fails` — Too short
- `test_private_key_flag_overrides_wallet` — Flag takes precedence
- `test_private_key_no_payment_needed` — 200 with `--private-key` (no payment)

**SSE/Streaming (2):**
- `test_sse_json_ndjson_schema` — `--sse-json` NDJSON output
- `test_sse_json_error_event` — SSE error event

**Curl Parity (10):**
- `test_dry_run_no_payment` — `--dry-run`
- `test_dry_run_price_json` — `--dry-run --price-json`
- `test_dry_run_price_json_toon_output` — `--dry-run --price-json -t`
- `test_referer_header` — `-e URL`
- `test_compressed_sets_accept_encoding` — `--compressed`
- `test_http2_flag_no_crash` — `--http2`
- `test_http1_1_flag_no_crash` — `--http1.1`
- `test_http2_http1_conflict` — `--http2 --http1.1` conflict
- `test_proxy_flag_no_crash` — `--proxy`
- `test_no_proxy_flag_succeeds` — `--no-proxy`

**JSON Error Schema (2):**
- `test_error_json_for_invalid_url` — `E_USAGE` for `ftp://`
- `test_error_json_for_connection_refused` — `E_NETWORK`

**Offline Mode (3):**
- `test_offline_flag_fails_fast` — `--offline`
- `test_offline_flag_json_error` — `--offline -j` → `E_NETWORK`
- `test_offline_flag_no_socket_opened` — No analytics event

**Analytics (7):**
- `test_analytics_event_sequence_success` — command_run→query_started→query_success
- `test_analytics_event_sequence_failure` — query_started→query_failure
- `test_analytics_url_query_params_redacted` — URL scrubbing
- `test_analytics_bearer_token_not_leaked` — Bearer redaction
- `test_analytics_basic_auth_not_leaked` — Basic auth redaction
- `test_analytics_custom_auth_header_not_leaked` — Custom Authorization header
- `test_analytics_private_key_env_not_leaked` — TEMPO_PRIVATE_KEY redaction

**Verbose Log Redaction (3):**
- `test_verbose_log_redacts_url_query_params`
- `test_verbose_log_redacts_bearer_in_stderr`
- `test_verbose_log_redacts_basic_auth_in_stderr`

**TOON Input/Output (5):**
- `test_toon_output_pretty_prints_json_response` — TOON output
- `test_toon_output_non_json_response_passthrough` — Non-JSON passthrough
- `test_toon_input_sets_content_type_json` — `--toon` → Content-Type: application/json
- `test_toon_input_invalid_data_errors` — Invalid TOON input
- `test_toon_and_json_input_conflict` — `--json` + `--toon` conflict

### `tempo-request/tests/cli.rs` — 2 tests ✅
- `request_help_shows_usage`
- `request_accepts_url`

### `tempo-request/tests/structured.rs` — 1 test ✅
- `query_json_and_toon_body_shape`

### `tempo-wallet/tests/split_wallet.rs` — 3 tests ✅
- `wallet_help_includes_identity_commands`
- `wallet_help_includes_mpp_commands`
- `wallet_rejects_query_command`

### `tempo-wallet/tests/sign_cli.rs` — 8 tests ✅
- `sign_help_shows_flags`
- `sign_dry_run_valid_challenge`
- `sign_dry_run_invalid_challenge`
- `sign_dry_run_unsupported_method`
- `sign_dry_run_missing_chain_id`
- `sign_no_wallet_configured`
- `sign_empty_stdin_fails`
- `sign_dry_run_reads_from_stdin`

### `tempo-wallet/tests/structured.rs` — 4 tests ✅
- `sessions_list_json_and_toon_have_expected_shape`
- `sessions_info_missing_json_and_toon_shape`
- `sessions_sync_empty_json_and_toon_shape`
- `services_json_and_toon_shapes`

---

## Part 4: Gap Analysis & New Tests Needed

### Priority 1 — Missing Command Coverage

#### `tempo-wallet` commands without integration tests:

| Command | Gap |
|---------|-----|
| `whoami` (no wallet) | Text + JSON + TOON output when not logged in |
| `whoami` (with wallet) | Needs mock RPC; text + structured output |
| `list` (no wallets) | Empty state text + structured |
| `list` (with wallets) | With keys.toml; text + structured |
| `create` | Creates keys.toml; verify file + output |
| `logout --yes` | Removes keys; verify cleanup |
| `keys list` (no keys) | Empty state |
| `keys list` (with keys) | Shows key details; needs mock RPC |
| `sessions close <url>` | With seeded session |
| `sessions close --all` | With seeded sessions |
| `sessions close --dry-run` | Preview without action |
| `sessions info <url>` (found) | With seeded session |
| `sessions list --state` | Filter by active/closing/all |
| `services --category` | Category filter with mock |
| `services --search` | Search filter with mock |
| `fund --dry-run` | Testnet: dry-run faucet |
| `completions` | Shell completions output |

#### `tempo-request` flags without integration tests:

| Flag | Gap |
|------|-----|
| `--head` / `-I` | HEAD request, no body returned |
| `--stream` | Raw streaming mode |
| `--sse` (passthrough) | SSE passthrough (vs `--sse-json`) |
| `--user-agent` / `-A` | Custom User-Agent header |
| `--connect-timeout` | TCP connection timeout |
| `--max-pay` | Payment cap enforcement |
| `--max-pay` + `--currency` | Cap with specific currency |
| `--save-receipt` | Receipt saved to file |
| `--write-meta` | Metadata written to file |
| `--remote-name` / `-O` | File named from URL path |
| `--data-urlencode` | URL-encoded POST data |
| `--max-redirs` | Redirect limit with `-L` |
| `--retry-http` | Retry on specific status codes |
| `--retry-after` | Retry-After header respect |
| `--retry-jitter` | Jitter on retries |
| `--bearer` | Actual Authorization header sent |
| `--insecure` / `-k` | TLS skip (smoke test) |
| `-G` (GET with data) | Data as query params |

### Priority 2 — Exit Code Verification

Currently tests use `output.status.success()` / `!success()`. Upgrade to exact exit code assertions using `assert_exit_code()`.

| Scenario | Expected Exit Code |
|----------|-------------------|
| Invalid URL scheme (`ftp://`) | 2 (`E_USAGE`) |
| Connection refused | 3 (`E_NETWORK`) |
| `--offline` mode | 3 (`E_NETWORK`) |
| 402 without WWW-Authenticate | 4 (`E_PAYMENT`) |
| 402 unsupported payment method | 4 (`E_PAYMENT`) |
| Invalid `--private-key` | 2 (`E_USAGE`) |
| Server 500 error | 1 (`E_GENERAL`) |
| Conflicting flags (`--http2 --http1.1`) | 2 (`E_USAGE`) |
| Invalid `--json` body | 2 (`E_USAGE`) |
| Missing URL argument | 2 (`E_USAGE`) |

### Priority 3 — Structured Output Gaps

#### `tempo-request` structured error output:

| Scenario | JSON | TOON | Gap |
|----------|------|------|-----|
| 402 payment error | ✅ | ❌ | TOON error schema |
| Network error (connection refused) | ✅ | ❌ | TOON error schema |
| Usage error (bad URL) | ✅ | ❌ | TOON error schema |
| `--offline` error | ✅ | ❌ | TOON error schema |

#### `tempo-wallet` structured output:

| Command | JSON | TOON | Gap |
|---------|------|------|-----|
| `sessions list` (empty) | ❌ | ❌ | Empty state shape |
| `sessions info` (found) | ❌ | ❌ | Session detail shape |
| `sessions close` | ❌ | ❌ | Close result shape |
| `services info` (not found) | ❌ | ❌ | Error shape |
| `whoami` (logged in) | ❌ | ❌ | Full status shape |
| `whoami` (not logged in) | ❌ | ❌ | Empty status shape |
| `list` (wallets) | ❌ | ❌ | Wallet list shape |
| `keys list` | ❌ | ❌ | Key list shape |
| `sign --dry-run` | ❌ | ❌ | Dry-run output shape |
| `--version` | ❌ | ❌ | Version JSON/TOON shape |

### Priority 4 — Global Flag Permutations

| Flag Combination | Gap |
|------------------|-----|
| `-v` + `-j` | Verbose + JSON (stderr empty, JSON on stdout) |
| `-v` + `-t` | Verbose + TOON |
| `-s` + `-j` | Silent + JSON |
| `--version -j` | Structured version |
| `--version -t` | TOON version |
| `--describe` | JSON schema output |
| `--color never` | No ANSI escapes |

### Priority 5 — Security & Edge Cases

| Test | Gap |
|------|-----|
| Private key NOT in verbose logs | `-v` with `--private-key` should not log key |
| Path traversal in `-o` | `../../etc/passwd` should fail |
| Path traversal in `-O` | URL path with `..` |
| Binary response body | Non-UTF-8 body with `-o` |

---

## Part 5: Implementation Plan

### Phase 0: Create `tempo-test-utils` crate *(prerequisite)*

1. Create `crates/tempo-test-utils/` with the module layout above
2. Move `TestConfigBuilder`, `write_test_files`, `get_combined_output` from both `common/mod.rs` files
3. Move `MockServer` (all variants), `MockRpcServer`, `mock_rpc_response` from `query.rs`
4. Move `seed_local_session`, `delete_sessions_db` from both `common/mod.rs` files
5. Add wallet constants (`HARDHAT_PRIVATE_KEY`, `MODERATO_DIRECT_KEYS_TOML`, `MODERATO_CHARGE_CHALLENGE`)
6. Add assertion helpers (`assert_exit_code`, `assert_clean_stderr`, `parse_json_stdout`, `parse_toon_stdout`, `assert_structured_error`, `run_all_formats`)
7. Add `PaymentTestHarness` builder
8. Add `MockServicesServer` (extracted from wallet's `structured.rs`)
9. Reduce each crate's `tests/common/mod.rs` to a thin re-export with binary-specific `test_command()`
10. Verify all existing tests still pass (`make test`)

### Phase 1: Retrofit existing tests *(~0 new tests, framework migration)*

1. Replace inline `MockServer` / `MockRpcServer` usage in `query.rs` with imports from `tempo-test-utils`
2. Replace 14 copies of the challenge string with `MODERATO_CHARGE_CHALLENGE`
3. Replace 5 copies of keys_toml blocks with `MODERATO_DIRECT_KEYS_TOML` / `MODERATO_KEYCHAIN_KEYS_TOML`
4. Replace payment test setup boilerplate with `PaymentTestHarness::charge()` where applicable
5. Convert `!success()` assertions to `assert_exit_code()` with exact codes
6. Verify all existing tests still pass (`make test`)

### Phase 2: Missing `tempo-request` flags *(~20 new tests)*

- Tests for `--head`, `--stream`, `--sse`, `-A`, `--connect-timeout`, `--save-receipt`, `--write-meta`, `-O`, `--data-urlencode`, `-G`, `--max-redirs`, `--retry-http`, `--retry-after`, `--bearer`, `--max-pay`

### Phase 3: Missing `tempo-wallet` commands *(~25 new tests)*

- Tests for `whoami`, `list`, `create`, `logout`, `keys list`, `sessions close/info/list --state`, `services --category/--search`, `sign --dry-run` structured, `--version` structured

### Phase 4: Structured output + global flag permutations *(~15 new tests)*

- TOON error schemas, format × verbosity combinations, `--describe`

### Phase 5: Security & edge cases *(~10 new tests)*

- Redaction, path traversal, binary bodies

---

## Estimated Final Test Count

| Category | Current | New | Total |
|----------|---------|-----|-------|
| `tempo-request` integration | 71 | ~45 | ~116 |
| `tempo-wallet` integration | 15 | ~25 | ~40 |
| **Total integration tests** | **86** | **~70** | **~156** |

Combined with the 352 unit tests, the workspace would have **~508 total tests**.

---

## What Adding a New Test Looks Like (After Framework)

### Simple HTTP test (3 lines of setup):
```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_new_flag() {
    let server = MockServer::start(200, vec![], "ok").await;
    let temp = TestConfigBuilder::new().build();
    let output = test_command(&temp).args(["--new-flag", &server.url("/test")]).output().unwrap();
    assert!(output.status.success());
}
```

### Payment flow test (2 lines of setup):
```rust
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_new_payment_feature() {
    let h = PaymentTestHarness::charge().await;
    let output = test_command(&h.temp).args([&h.url("/api")]).output().unwrap();
    assert!(output.status.success());
}
```

### Three-format structured test (1 call):
```rust
#[test]
fn test_new_command_all_formats() {
    let temp = TestConfigBuilder::new().build();
    let (text, json_out, json, toon_out, toon) = run_all_formats(&temp, "tempo-wallet", &["new-cmd"]);
    assert!(text.status.success());
    assert_clean_stderr(&json_out);
    assert_clean_stderr(&toon_out);
    assert_eq!(json["field"], toon["field"]);
}
```

### New extension binary (2 files):
```rust
// crates/tempo-foo/tests/common/mod.rs
pub use tempo_test_utils::*;
pub fn test_command(temp_dir: &tempfile::TempDir) -> std::process::Command {
    tempo_test_utils::test_command("tempo-foo", temp_dir)
}

// crates/tempo-foo/tests/smoke.rs
mod common;
use common::*;

#[test]
fn help_works() {
    let temp = TestConfigBuilder::new().build();
    test_command(&temp).arg("--help").assert().success();
}
```
