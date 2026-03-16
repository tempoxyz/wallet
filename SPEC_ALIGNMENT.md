# Spec Alignment Plan

Align codebase terminology, DB structure, and business logic with
[draft-tempo-session-00](https://paymentauth.org/draft-tempo-session-00.txt).

**Constraint:** CLI keeps `sessions` as the product name. Internal code and
storage use spec terminology (`channel`, `payee`, etc.).

## Decisions (2026-03-15)

1. **Non-streaming insufficient-balance recovery is automatic.**
   The client performs `topUp` and replays the paid request when the server
   returns `402` with
   `type=https://paymentauth.org/problems/session/insufficient-balance`.
2. **Missing receipts are warning-only at runtime.**
   Missing `Payment-Receipt` on successful paid responses logs a warning and
   does not fail the request. Spec conformance is enforced in tests.
3. **`tempo wallet sign` remains charge-only.**
   Session-intent signing support is intentionally out of scope for this
   alignment pass.

## Implementation Status (Current)

- [x] Task 1 — Foundation: types, schema, field renames, error variants
- [x] Task 2 — Store public API: function renames + new queries
- [x] Task 3 — Request flow refactor: reuse, persist, and reuse-check
- [x] Task 4 — Wallet commands: migrate off `session_key`
- [x] Task 5 — Strict protocol field parsing (§4)
- [x] Task 6 — RFC 9457 Problem Details parsing (§10)
- [x] Task 7 — On-chain validation + server-suggested channelId on reuse (§6.2)
- [x] Task 8 — Active session guard (§1.3, §12.4)
- [x] Task 9 — Receipt validation + acceptedCumulative persistence (§12.6, §12.4)
- [x] Task 10 — feePayer support (§7)
- [x] Task 11 — Streaming topUp + strict `payment-need-voucher` validation (§8.3.2, §11.6)
- [x] Task 11b — Non-streaming auto-topUp on insufficient-balance (§11.2, §10.5)
- [x] Task 12 — Idempotency-Key header handling (§11.4)
- [x] Task 13 — HEAD for mid-stream voucher top-ups (§12.5)
- [x] Task 13b — Observable voucher/topUp transport outcomes
- [x] Task 14 — Test fixtures + existing test updates (baseline updates)
- [x] Task 15 — Doc comments, ARCHITECTURE.md, deviation docs
- [x] Task 15b — Receipt strictness policy (reference mode)
- [x] Task 15c — Dependency guarantees (`mpp` crate)
- [x] Task 15d — Scope Boundary: Client vs Server MUSTs
- [x] Task 16 — `make check`
- [x] Task 15e — removed from scope for this alignment pass

---

## Task 1 — Foundation: types, schema, field renames, error variants

Rename all persisted/internal types from session → channel terminology,
migrate the DB schema, rename `recipient` → `payee`, store `payer` as
raw address (not DID), rename `currency` → `token` for internal types,
and rename error variants. This is one atomic task because the type
renames, schema changes, and field renames are all interdependent — they
touch the same structs and the same DB table.

### 1a. Type renames: `Session*` → `Channel*`

The spec distinguishes *channel* (on-chain, persistent) from *session*
(HTTP interaction on a channel, ephemeral — §12.4). What we persist is
channel state.

| Current | New |
|---|---|
| `SessionRecord` | `ChannelRecord` |
| `SessionStatus` | `ChannelStatus` |
| `SessionState` (tempo-request `session/mod.rs`) | `ChannelState` |
| `SessionContext` (tempo-request `session/mod.rs`) | `ChannelContext` |
| `SessionLock` | `ChannelLock` |
| `SessionStoreDiagnostics` | `ChannelStoreDiagnostics` |
| `SessionStoreResult<T>` (storage.rs) | `ChannelStoreResult<T>` |
| `SessionItem` (render.rs) | `ChannelItem` |
| `SessionListResponse` (render.rs) | `ChannelListResponse` |
| `SessionStateArg` (args.rs) | `ChannelStateArg` |
| `InvalidSessionStatusError` | `InvalidChannelStatusError` |
| `type SessionResult<T>` alias (5 files in tempo-common) | `ChannelResult<T>` |

The `SessionResult<T>` alias in `tempo-wallet/src/commands/sessions/util.rs`
stays as-is (CLI product term).

### 1b. Field rename: `recipient` → `payee`

The spec uses `payee` for the on-chain field (§5.1) and `recipient` only
in the challenge request JSON (§6.1). Our persisted record and internal
types should use the on-chain term.

| Current | New | Notes |
|---|---|---|
| `SessionRecord.recipient` | `ChannelRecord.payee` | DB column too |
| `SessionContext.recipient` (mod.rs L40) | `ChannelContext.payee` | Struct field + all construction sites |
| `is_session_reusable(..., recipient)` | `is_channel_reusable(..., payee)` | |
| DB column `recipient` | `payee` | |
| `persist.rs` write of `ctx.recipient` | `ctx.payee` | `format!("{:#x}", ctx.payee)` |

**Keep `recipient`** in challenge parsing code (`flow.rs` lines ~206-211)
since that matches the wire format (`request.recipient`). The local
variable `recipient` in `flow.rs` is passed positionally to
`build_open_calls` (which already uses `payee` as its parameter name in
`tx.rs`), so the local can stay as `recipient`. Convert to `payee` when
building `ChannelContext` (flow.rs construction sites at lines ~358, ~482).

### 1c. Payer storage: DID → raw address

The spec (§5.1) uses a raw address for `payer`. Currently we store a DID
string (`did:pkh:eip155:{chainId}:{address}`). Store the raw address and
derive the DID at use time.

| Current | New |
|---|---|
| `payer: "did:pkh:eip155:4217:0x..."` | `payer: "0x..."` |

Any code that needs the DID must derive it:
`format!("did:pkh:eip155:{}:{}", record.chain_id, record.payer)`.

### 1d. DID derivation guardrail for payer vs authorized_signer (§3 addendum)

After migrating `payer` from DID to raw address, the `persist.rs`
storage site must explicitly store the **payer wallet address**
(`signer.from`), not the `authorized_signer` address
(`signer.signer.address()`). These can differ when key delegation is
used.

Currently `persist.rs:48` stores `ctx.did.to_string()` which contains
`from` (the payer). After migration it must store
`format!("{:#x}", ctx.signer_from)` or equivalent, using the payer's
address — not the signing key's address.

1. Ensure `ChannelContext` carries an explicit `payer: Address` field
   (the `from` address on the `Signer` struct), separate from the
   `signer` (the `PrivateKeySigner` whose `address()` is the
   authorized_signer).
2. In `persist_channel`, store `format!("{:#x}", ctx.payer)` in the
   `payer` column.
3. In `cooperative.rs`, derive the DID as:
   `format!("did:pkh:eip155:{}:{:#x}", record.chain_id, record.payer)`
   where `record.payer` is the raw payer address.

### 1e. Field rename: `currency` → `token` for internal types (§5.1)

The spec uses `token` for the on-chain field (§5.1) and `currency` only
in the challenge wire format (§6.1).

Keep `currency` in the challenge/wire layer (matches `request.currency`).
For the persisted `ChannelRecord` and internal types, rename:

| Current | New | Notes |
|---|---|---|
| `ChannelRecord.currency` | `ChannelRecord.token` | On-chain term |
| `ChannelContext.currency` | `ChannelContext.token` | Internal struct |
| DB column `currency` | `token` | Schema alignment |

Wire-format parsing continues to use `session_req.currency` from the
challenge, converting to `token` when building internal types.

### 1f. DB schema: `sessions` → `channels`, `channel_id` as PK

| Change | Detail |
|---|---|
| Table: `sessions` → `channels` | Rename |
| PK: `key TEXT` (origin-derived) → `channel_id TEXT` | On-chain identity as natural key |
| Drop: `UNIQUE` on `origin` | Multiple channels per origin now supported |
| Add: `CREATE INDEX idx_channels_origin ON channels(origin)` | Lookup performance |
| Column: `recipient` → `payee` | Spec alignment (§5.1) |
| Column: `currency` → `token` | Spec alignment (§5.1) |
| File: `sessions.db` → `channels.db` | Match table name |

New schema:
```sql
CREATE TABLE IF NOT EXISTS channels (
    channel_id        TEXT PRIMARY KEY,
    version           INTEGER NOT NULL DEFAULT 1,
    origin            TEXT NOT NULL,
    request_url       TEXT NOT NULL DEFAULT '',
    chain_id          INTEGER NOT NULL,
    escrow_contract   TEXT NOT NULL,
    token             TEXT NOT NULL,
    payee             TEXT NOT NULL,
    payer             TEXT NOT NULL,
    authorized_signer TEXT NOT NULL,
    salt              TEXT NOT NULL,
    deposit           TEXT NOT NULL,
    cumulative_amount TEXT NOT NULL,
    challenge_echo    TEXT NOT NULL,
    state             TEXT NOT NULL DEFAULT 'active',
    close_requested_at INTEGER NOT NULL DEFAULT 0,
    grace_ready_at     INTEGER NOT NULL DEFAULT 0,
    created_at        INTEGER NOT NULL,
    last_used_at      INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_channels_origin ON channels(origin);
```

Since `channels.db` is a new file (no users have it), the legacy schema
detection logic (`table_needs_reset`, `EXPECTED_COLUMN_COUNT`, and
the `test_table_needs_reset_*` tests) can be **removed entirely**.
`init_schema` simply creates the `channels` table if it doesn't exist.

### 1g. `normalize_persisted_identity()` update

Rename internal references from `recipient` to `payee`:
- Error message: `"invalid session currency or recipient address"` →
  `"invalid channel token or payee address"`

### 1h. Error variant renames

| Current | New |
|---|---|
| `PaymentError::SessionPersistence` | `PaymentError::ChannelPersistence` |
| `PaymentError::SessionPersistenceSource` | `PaymentError::ChannelPersistenceSource` |
| `PaymentError::SessionPersistenceContextSource` | `PaymentError::ChannelPersistenceContextSource` |

Error messages: `"Session persistence error"` → `"Channel persistence error"`

Error message strings in `storage.rs` (store_error call sites): rename all
operation strings from `"session"` → `"channel"` equivalents:
`"open channels database"`, `"set channels database permissions"`,
`"configure channels database pragmas"`, `"create channels schema"`,
`"drop outdated channels table"`, `"ensure channel wallet dir"`,
`"serialize channel integer field"`, `"prepare channel load query"`,
`"save channel"`, `"delete channel"`, `"load channel"`, `"list channels"`,
`"update channel close state"`, `"prepare channels list query"`, etc.

### 1i. Analytics event renames

| Current | New |
|---|---|
| `SESSION_STORE_DEGRADED` | `CHANNEL_STORE_DEGRADED` |
| `SessionStoreDegradedPayload` | `ChannelStoreDegradedPayload` |
| `session_id` in `PaymentResult` | `channel_id` |
| `session_id` in `PaymentSuccessPayload` | `channel_id` |
| `"session store degraded"` event string | `"channel store degraded"` |
| `SESSION_RECOVERED` event (sync.rs) | `CHANNEL_RECOVERED` / `"channel recovered"` |

### Files affected (combined)
- `crates/tempo-common/src/error.rs`
- `crates/tempo-common/src/analytics.rs`
- `crates/tempo-common/src/cli/runner.rs`
- `crates/tempo-common/src/payment/session/store/model.rs`
- `crates/tempo-common/src/payment/session/store/storage.rs`
- `crates/tempo-common/src/payment/session/store/lock.rs`
- `crates/tempo-common/src/payment/session/store/mod.rs`
- `crates/tempo-common/src/payment/session/mod.rs`
- `crates/tempo-common/src/payment/session/close/mod.rs`
- `crates/tempo-common/src/payment/session/close/cooperative.rs`
- `crates/tempo-common/src/payment/session/close/onchain.rs`
- `crates/tempo-common/src/payment/session/channel.rs`
- `crates/tempo-common/src/payment/session/tx.rs`
- `crates/tempo-request/src/payment/session/mod.rs`
- `crates/tempo-request/src/payment/session/flow.rs`
- `crates/tempo-request/src/payment/session/persist.rs`
- `crates/tempo-request/src/payment/session/streaming.rs`
- `crates/tempo-request/src/payment/session/voucher.rs`
- `crates/tempo-request/src/payment/types.rs`
- `crates/tempo-request/src/analytics.rs`
- `crates/tempo-request/src/query/analytics.rs`
- `crates/tempo-request/src/query/mod.rs`
- `crates/tempo-request/src/payment/charge.rs`
- `crates/tempo-wallet/src/args.rs`
- `crates/tempo-wallet/src/app.rs`
- `crates/tempo-wallet/src/commands/sessions/mod.rs`
- `crates/tempo-wallet/src/commands/sessions/list.rs`
- `crates/tempo-wallet/src/commands/sessions/close.rs`
- `crates/tempo-wallet/src/commands/sessions/sync.rs`
- `crates/tempo-wallet/src/commands/sessions/render.rs`
- `crates/tempo-wallet/src/commands/keys.rs`
- `crates/tempo-wallet/src/wallet/render.rs`
- `crates/tempo-test/src/fixture.rs`

---

## Task 2 — Store public API: function renames + new queries

Depends on Task 1 (types and schema must exist first).

Rename all public store functions, remove `session_key()`, add
`find_reusable_channel()` and `load_channel_by_origin()`.

### Function renames

| Current | New |
|---|---|
| `session_key(url)` | **Remove** — no longer needed |
| `load_session(key)` | `load_channel(channel_id)` |
| `save_session(record)` | `save_channel(record)` |
| `delete_session(key)` | `delete_channel(channel_id)` |
| `list_sessions()` | `list_channels()` |
| `load_session_by_channel_id()` | **Remove** — `load_channel` now takes channel_id |
| `delete_session_by_channel_id()` | **Remove** — `delete_channel` now takes channel_id |
| `update_session_close_state_by_channel_id()` | `update_channel_close_state()` |
| `acquire_origin_lock(key)` | `acquire_origin_lock(origin)` (keep name — lock is origin-based) |
| `take_store_diagnostics()` | `take_channel_store_diagnostics()` |
| `close_session_from_record` (close/mod.rs) | `close_channel_from_record` |
| `persist_session` (persist.rs) | `persist_channel` |
| `session_store_error` (flow.rs helper) | `channel_store_error` |
| `session_reuse_preserved_error` (flow.rs) | `channel_reuse_preserved_error` |
| `session_store_diagnostics` variable (cli/runner.rs) | `channel_store_diagnostics` |
| `store as session_store` import (tempo-common close files) | `store as channel_store` |
| `decode_session_state` (storage.rs) | `decode_channel_state` |
| `map_session_row` (storage.rs) | `map_channel_row` |
| `is_malformed_session_row_error` (storage.rs) | `is_malformed_channel_row_error` |

The `close_session_from_record` re-export in `session/mod.rs` must also
be updated to `close_channel_from_record`, along with all import sites
(e.g., wallet `close.rs` line 8).

### New function: `find_reusable_channel()`

```rust
pub fn find_reusable_channel(
    origin: &str,
    payer: &str,
    escrow_contract: Address,
    token: &str,
    payee: &str,
    chain_id: u64,
) -> Result<Option<ChannelRecord>>
```

Query:
```sql
SELECT ... FROM channels
WHERE origin = ?1 AND payer = ?2 AND escrow_contract = ?3
  AND token = ?4 AND payee = ?5 AND chain_id = ?6
  AND state = 'active'
ORDER BY last_used_at DESC LIMIT 1
```

### New function: `load_channel_by_origin()`

After removing `session_key`, wallet commands that look up channels by
URL/origin need a replacement:

```rust
pub fn load_channel_by_origin(origin: &str) -> Result<Option<ChannelRecord>>
```

Query:
```sql
SELECT ... FROM channels
WHERE origin = ?1
ORDER BY last_used_at DESC LIMIT 1
```

Used by `close_by_url`, `sync_origin`, `dry_run_close` in wallet commands.

### Lock refactor

Keep origin-based locking for the critical section. Rename struct
`SessionLock` → `ChannelLock`, but keep the function name
`acquire_origin_lock` since the lock key is the origin, not a channel_id.

### Files affected
- `crates/tempo-common/src/payment/session/store/storage.rs`
- `crates/tempo-common/src/payment/session/store/mod.rs`
- `crates/tempo-common/src/payment/session/store/lock.rs`
- `crates/tempo-common/src/payment/session/mod.rs`
- `crates/tempo-common/src/payment/session/close/mod.rs` — re-export
- `crates/tempo-common/src/payment/session/close/cooperative.rs` — import alias
- `crates/tempo-common/src/payment/session/close/onchain.rs` — import alias
- `crates/tempo-common/src/cli/runner.rs` — diagnostics variable
- `crates/tempo-request/src/payment/session/persist.rs`
- `crates/tempo-request/src/payment/session/flow.rs`

---

## Task 3 — Request flow refactor: reuse, persist, and reuse-check

Depends on Task 2 (new store API must exist).

Replace the `session_key` + `load_session` + `is_session_reusable`
pattern with `find_reusable_channel`, refactor persist to use
`channel_id`, and update the reuse check.

### Reuse: use `find_reusable_channel`

Replace:
```rust
let session_key = session::session_key(url);
let existing = session::load_session(&session_key)?;
let reuse = existing.as_ref().is_some_and(|r| is_session_reusable(...));
```

With:
```rust
let existing = session::find_reusable_channel(
    &origin, &payer_address, escrow_contract, &token_hex, &payee_hex, chain_id
)?;
// If Some, reuse. If None, open new channel.
```

This eliminates `is_session_reusable` entirely — the SQL query does the
filtering (including `AND state = 'active'`, which the current function
lacks — a `Closing` channel currently passes the reuse check).

**Important:** The `payer` parameter must be the **raw address**
(`format!("{from:#x}")`), not the DID string. After Task 1c, persisted
`payer` is a raw address, so the comparison column matches raw-to-raw.

### Persist: use `channel_id` for load/save

Replace:
```rust
let session_key = session_key(ctx.url);
let existing = load_session(&session_key)?
    .filter(|r| r.channel_id == state.channel_id);
```

With:
```rust
let existing = load_channel(&state.channel_id_hex())?;
```

### CLI output terminology

Update internal log messages and error strings:

| Current | New | Scope |
|---|---|---|
| `"Session persisted"` log | `"Channel persisted"` | Debug log |
| `"Reusing session"` log | `"Reusing channel"` | Debug log |
| `"session state preserved"` error context | `"channel state preserved"` | Error message |
| `"session list"` / `"session state"` comments | `"channel list"` / `"channel state"` | Code comments |

Keep all user-facing CLI text (`"sessions"`, `"No sessions."`, etc.).

### Files affected
- `crates/tempo-request/src/payment/session/flow.rs`
- `crates/tempo-request/src/payment/session/persist.rs`
- `crates/tempo-common/src/payment/session/close/onchain.rs`

---

## Task 4 — Wallet commands: migrate off `session_key`

Depends on Task 2 (`load_channel_by_origin` must exist).

Wallet commands that look up channels by URL/origin must migrate from
`session_key(target)` + `load_session(&key)` to `load_channel_by_origin()`:

- **`close_by_url` (`close.rs`)** — replace with
  `load_channel_by_origin(origin)` where `origin` is extracted from
  the target URL using `url::Url::parse(target).origin()`.
- **`dry_run_close` (`close.rs`)** — same pattern.
- **`sync_origin` (`sync.rs`)** — replace with
  `load_channel_by_origin(origin_input)`.
- **`close_all_sessions` (`close.rs`)** — replace
  `session_key(&session.origin)` deletion with `delete_channel(channel_id)`.

### Files affected
- `crates/tempo-wallet/src/commands/sessions/close.rs`
- `crates/tempo-wallet/src/commands/sessions/sync.rs`

---

## Task 5 — Strict protocol field parsing (§4)

No dependencies on other tasks; can be done any time after Task 1.

The spec §4 defines normative encoding rules. Replace all `unwrap_or(0)`
/ `unwrap_or_default()` on protocol fields with explicit error handling.

Currently `streaming.rs` L194 does:
```rust
let required: u128 = nv.required_cumulative.parse().unwrap_or(0);
```

This silently treats malformed `requiredCumulative` as `0`, which could
cause the client to skip a voucher obligation or sign a voucher for 0.

Replace with:
```rust
let required: u128 = nv.required_cumulative.parse().map_err(|_| {
    PaymentError::PaymentRejected {
        reason: format!(
            "malformed requiredCumulative in payment-need-voucher: '{}'",
            nv.required_cumulative
        ),
        status_code: 0,
    }
})?;
```

Apply the same pattern to `nv.deposit` parsing (when added for topUp)
and receipt field parsing (when added for receipts).

Also implement spec default handling for optional `methodDetails.chainId`
(§6.2 / Table 12): when absent, default to `42431` instead of failing.

Add a single normalization pass for all string-comparison boundaries that
carry hex identifiers:
- Parse as typed values (`Address`, `B256`) where possible and compare typed values.
- Where typed parsing is not available, normalize to lowercase `0x...` before comparison.
- Apply to reuse checks, server-suggested `channelId`, and persisted identity fields.

`suggested_deposit` in the challenge (flow.rs L276–277) can keep
`unwrap_or(base_units)` — `suggestedDeposit` is OPTIONAL per §6.1.

### Files affected
- `crates/tempo-request/src/payment/session/streaming.rs`
- `crates/tempo-request/src/payment/session/flow.rs`
- `crates/tempo-common/src/payment/session/store/model.rs`

---

## Task 6 — RFC 9457 Problem Details parsing (§10)

No strict dependencies; can be done after Task 1.

The spec §10 defines problem types for session errors:

| Problem Type URI | Meaning |
|---|---|
| `.../session/channel-not-found` | Unknown or unfunded channel |
| `.../session/insufficient-balance` | Insufficient authorized balance |
| `.../session/challenge-not-found` | Unknown or expired challenge |
| `.../session/delta-too-small` | Voucher increase below policy minimum |
| `.../session/amount-exceeds-deposit` | Voucher exceeds channel deposit |
| `.../session/channel-finalized` | Channel already closed |
| `.../session/invalid-signature` | Signature format/recovery failed |
| `.../session/signer-mismatch` | Signer is not authorized for channel |

Currently `extract_json_error()` only extracts `error`/`message`/`detail`
free-text fields. The 410 retry in `open.rs` relies on brittle substring
matching (`"channel not funded"`, `"Channel Not Found"`).

1. Add a `ProblemDetails` struct with `problem_type`, `title`, `status`,
   `detail` fields and optional extension fields used by this intent
   (for example `requiredTopUp`, `channelId`).
2. Add `parse_problem_details(body: &str) -> Option<ProblemDetails>`.
3. Update `extract_json_error()` to try `parse_problem_details()` first.
4. Update `open.rs` 410 retry logic to match on problem type URI instead
   of substring matching.
5. Add helpers for matching specific session problem types used by client
   recovery paths and terminal failures (`channel-not-found`,
   `challenge-not-found`, `insufficient-balance`, `delta-too-small`,
   `amount-exceeds-deposit`, `channel-finalized`, `invalid-signature`,
   `signer-mismatch`).
6. Route these typed problems to deterministic client behavior:
   retry/open-new/top-up when recoverable, immediate structured error when not.
7. **Channel invalidation on `channel-not-found` / `channel-finalized`
   (§13.12):** when a locally-persisted channel receives a 410 with
   `channel-not-found` or `channel-finalized` (including after chain
   reorg), mark the local record as unusable (`state = Closed` or delete)
   and fall through to open a new channel. Do not retry reuse of an
   invalidated channel indefinitely.

### Files affected
- `crates/tempo-common/src/payment/classify.rs`
- `crates/tempo-request/src/payment/session/open.rs`
- `crates/tempo-request/src/payment/session/flow.rs`

---

## Task 7 — On-chain validation + server-suggested channelId on reuse (§6.2)

Depends on Task 3 (reuse flow must use `find_reusable_channel`).

### On-chain `settled` validation (§6.2, MUST)

The spec §6.2 says: *"Client MUST verify `channel.deposit -
channel.settled >= amount` before resuming."* The current reuse path
only checks local DB fields — it never queries on-chain `settled` state.

1. After `find_reusable_channel` returns a candidate, query the escrow
   contract via `get_channel_on_chain(provider, escrow, channel_id)`.
2. Verify:
   - `deposit - settled >= amount` (sufficient available balance)
   - `closeRequestedAt == 0` (no pending close)
   - `!finalized` (channel still open)
3. If verification fails, skip reuse and open a new channel.
4. Update the local record's `deposit` from on-chain state to keep
   the cache fresh. Use on-chain `deposit - settled` as the available
   balance ceiling for the reuse path (not just local `record.deposit`).

### Respect server-suggested `methodDetails.channelId` (§6.2)

A reference implementation should respect the server's channel suggestion
for correct server-side accounting.

1. Parse `methodDetails.channelId` from the decoded `SessionRequest`.
2. If present: attempt local load by that exact `channelId` and always
   validate against on-chain state. If local record is missing but on-chain
   channel is valid and identity fields match (`payer`, `payee`, `token`,
   `escrow`, `chainId`, `authorizedSigner`), seed local cache from chain data
   and reuse; otherwise
   fall through to DB-based reuse or new channel.
3. If absent: use DB-based reuse logic (no change).

### Files affected
- `crates/tempo-request/src/payment/session/flow.rs`

---

## Task 8 — Active session guard (§1.3, §12.4)

Depends on Task 3 (reuse flow must be refactored first).

The spec §1.3 says: *"A channel supports one active session at a time."*
Currently the origin lock is dropped **before** `send_session_request()`
and SSE streaming. A second worker can reuse the same channel while the
first stream is still active.

**Simplest approach:** Hold the origin lock for the entire request
duration instead of just the reuse/open decision. This serializes
concurrent requests to the same origin, which is correct per §1.3.

**Alternative:** Add an `active` boolean column to `channels`, set it
during request, clear on completion, filter in `find_reusable_channel`.

### Files affected
- `crates/tempo-common/src/payment/session/store/storage.rs` — schema or lock
- `crates/tempo-request/src/payment/session/flow.rs` — lock lifetime

---

## Task 9 — Receipt validation and `acceptedCumulative` persistence (§12.6, §12.4)

Depends on Task 3 (persist flow must use channel_id).

The spec §12.6 requires `Payment-Receipt` on every successful paid
request. §12.4 says `highestVoucherAmount` is the source of truth for
the next voucher's minimum. Currently the client ignores receipts almost
entirely.

1. **Parse receipts on all successful paid responses:**
   - After open: extend existing `Payment-Receipt` parsing to extract
     `acceptedCumulative`.
   - After non-SSE session request: parse `Payment-Receipt` header.
   - For SSE responses: parse and persist `Payment-Receipt` from the
     initial HTTP response headers before entering the event stream.
   - During SSE: persist `acceptedCumulative` from `PaymentReceipt` event.
   - For voucher/topUp submissions: parse `Payment-Receipt` from successful
     update responses when available and persist `acceptedCumulative`.

2. **Validate receipt before persisting:** verify `method == "tempo"`,
   `intent == "session"`, `status == "success"`, and `channelId` matches
   the active channel. Discard receipts that fail validation (log warning).
   This prevents a malformed or wrong-channel receipt from corrupting
   persisted channel state.

3. **Persist `acceptedCumulative`:** update the channel record's
   `cumulative_amount` to `max(local, receipt.accepted_cumulative)`.
   Updates MUST be monotonic — never decrease local cumulative from a
   late or out-of-order response.

4. **Use `acceptedCumulative` as reuse baseline:** the initial cumulative
   for a new session should be `max(record.cumulative_amount, amount)`.

5. **Warn on missing receipts:** log a warning on 2xx without
   `Payment-Receipt`. Do NOT fail.
5. **Trailer support:** for chunked paid responses, support final receipt
   parsing from HTTP trailers when the server uses trailer delivery.
6. Add integration assertions that compliant mock servers return
   `Payment-Receipt` on successful paid responses; missing receipts remain
   warning-only in runtime behavior.

### Files affected
- `crates/tempo-request/src/payment/session/flow.rs`
- `crates/tempo-request/src/payment/session/streaming.rs`
- `crates/tempo-request/src/payment/session/persist.rs`

---

## Task 10 — feePayer support (§7, MUST)

Depends on Task 1 (type renames) and Task 3 (flow refactor).

The spec §7.1/§7.5: when `methodDetails.feePayer` is `true`, the client
**MUST** sign with `fee_payer_signature` set to `0x00` and `fee_token`
empty. Currently `tx.rs` hardcodes `fee_payer: false`.

1. Parse `methodDetails.feePayer` from the decoded `SessionRequest`.
2. Plumb the boolean through to `create_tempo_payment_from_calls` and
   `resolve_and_sign_tx`.
3. When `fee_payer = true`: set `fee_payer: true` in `TempoTxOptions`,
   set `fee_token` to RLP null / empty address.
4. When `fee_payer = false` (default): current behavior is correct.

### Files affected
- `crates/tempo-request/src/payment/session/flow.rs`
- `crates/tempo-common/src/payment/session/tx.rs`
- `crates/tempo-request/src/payment/session/open.rs`
- `crates/tempo-request/src/payment/session/streaming.rs`

---

## Task 11 — topUp implementation (§8.3.2, §11.6)

Depends on Task 5 (strict parsing), Task 10 (feePayer plumbing).

The spec requires that when `requiredCumulative > deposit` during
streaming, the client MUST submit `action="topUp"` to add funds.
Currently the code clamps voucher amounts to `ctx.deposit`, so the
stream silently ends when deposit is exhausted.

### Prerequisites

Verify that `mpp::SessionCredentialPayload` has a `TopUp` variant. If
not, an `mpp` crate update is required first.

### Behavior

1. During SSE streaming, when `payment-need-voucher` arrives:
   - Parse **all four** required fields strictly (per Task 5):
     `channelId`, `acceptedCumulative`, `deposit`, `requiredCumulative`.
   - **Validate `channelId` matches the active channel.** If it does not,
     abort the stream with a protocol error — the event is for a
     different channel.
   - Use `acceptedCumulative` as the server's floor:
     `next = max(local_cumulative, acceptedCumulative, requiredCumulative)`.
   - Compare `requiredCumulative` against **`nv.deposit`** (server-provided
     on-chain deposit), NOT local `ctx.deposit`.
   - When `requiredCumulative > nv.deposit`:
     - Build `approve` + `escrow.topUp(channelId, additionalDeposit)`.
     - Send credential with `action="topUp"`, `type="transaction"`,
       `channelId`, `transaction`, `additionalDeposit`.
     - Update `ctx.deposit` to `nv.deposit + additionalDeposit`.
     - If the topUp target channel had `state = Closing` locally
       (§5.3.3: topUp cancels pending close), transition local state
       back to `Active` and clear `close_requested_at` / `grace_ready_at`.
     - Then send the voucher for `requiredCumulative`.

2. `additionalDeposit` sized same as initial deposit (suggested_deposit
   or 1 token, clamped to 50% of remaining balance), **but always at least**
   `requiredCumulative - nv.deposit` so one top-up satisfies the immediate
   server requirement.

3. Reuse `open::create_tempo_payment_from_calls` for tx building.

4. **Challenge for topUp:** Reuse `ctx.echo` from initial 402. Per spec
   §8.3.2, fresh challenge only needed for top-ups *outside* active
   streaming. Before reuse, check `expires` from the cached echo — if
   expired, proactively fetch a fresh challenge (HEAD to protected
   resource) rather than waiting for a `challenge-not-found` rejection.
   If top-up fails with `challenge-not-found`, fetch a fresh challenge
   and retry top-up once.

5. **Also fix non-topUp voucher path:** Even when `requiredCumulative <=
   deposit`, use `nv.deposit` (not `ctx.deposit`) as the clamp ceiling.

### Files affected
- `crates/tempo-request/src/payment/session/streaming.rs`
- `crates/tempo-request/src/payment/session/voucher.rs`
- `crates/tempo-request/src/payment/session/open.rs`
- `crates/tempo-request/src/payment/session/mod.rs`
- `crates/tempo-request/src/payment/session/persist.rs`

---

## Task 11b — Non-streaming auto-topUp on `insufficient-balance` (§11.2, §10.5)

Depends on Task 6 (Problem Details parsing), Task 10 (feePayer), and Task 11
(topUp primitives).

For non-SSE paid requests, the client should recover automatically when the
server indicates insufficient authorized balance.

### Behavior

1. In non-streaming paid request flow, when response is `402` with Problem
   Details type `.../session/insufficient-balance`, parse `requiredTopUp`
   strictly as a positive integer amount.
2. Build and send `action="topUp"` to the same protected resource URI with
   `additionalDeposit >= requiredTopUp` (respecting fee sponsorship rules from
   Task 10).
3. After successful top-up acceptance, replay the original paid request
   automatically (same method/body/headers plus updated payment credential).
4. If top-up fails with `challenge-not-found`, obtain a fresh challenge from
   the protected resource and retry top-up once.
5. If recovery fails, return a structured `PaymentRejected` error that includes
   problem `type` and `detail` text.

### Files affected
- `crates/tempo-request/src/payment/session/flow.rs`
- `crates/tempo-request/src/payment/session/open.rs`
- `crates/tempo-request/src/payment/session/voucher.rs`
- `crates/tempo-common/src/payment/classify.rs`

---

## Task 12 — Idempotency-Key header (§11.4, SHOULD)

No strict dependencies; can be done after Task 3.

The spec §11.4: *"Clients SHOULD include an Idempotency-Key header on
paid requests."*

1. Generate a UUID v4 `Idempotency-Key` for each logical paid request,
   including open/voucher/topUp/close credentials **and** the primary paid
   resource request carrying `Authorization: Payment`.
2. Include it as an HTTP header.
3. On retry (e.g., `send_open_with_retry`), reuse the same key.
4. For stalled SSE voucher re-posts and non-streaming top-up retries, keep the
   same idempotency key per logical operation (new key only for a new logical
   voucher/top-up operation).
5. For non-streaming request replay after auto-topUp, preserve the same
   idempotency key for the replayed logical request.
6. **Handle duplicate voucher 200 OK (§10.4):** when a voucher resubmission
   returns 200 with `cumulativeAmount <= highestAccepted`, treat as a no-op
   success. Persist any returned `Payment-Receipt` (monotonic update only).
   Do not increment local state or sign a new voucher.

### Files affected
- `crates/tempo-request/src/payment/session/flow.rs`
- `crates/tempo-request/src/payment/session/streaming.rs`
- `crates/tempo-request/src/payment/session/open.rs`

---

## Task 13 — HEAD for mid-stream voucher top-ups (§12.5, MAY)

No strict dependencies; can be done after Task 3.

The spec §12.5: *"For voucher-only updates (no response body needed),
clients MAY use HEAD requests."* Servers are only `SHOULD` support for
HEAD in this path, so the client should degrade gracefully.

Update `post_voucher` transport behavior:
1. Try `HEAD` first for voucher-only updates.
2. If server returns method-not-supported behavior (e.g., `405`, `501`),
   or a transport/proxy incompatibility is detected, retry with `POST`.
3. Emit debug logs that indicate whether `HEAD` or fallback `POST` was used.
4. Keep transport behavior decoupled from stream reads, but do not treat
   voucher update responses as opaque.
5. **Dedicated transport client (§12.5 SHOULD):** use a separate
   `reqwest::Client` instance (or connection pool) for voucher/topUp
   submissions, distinct from the primary streaming client. This avoids
   head-of-line blocking on HTTP/1.1 and naturally supports HTTP/2
   multiplexing when available. Configure the voucher client with
   `http2_prior_knowledge` or `http2_adaptive_window` where the server
   supports it.

### Files affected
- `crates/tempo-request/src/payment/session/streaming.rs`
- `crates/tempo-request/src/payment/session/flow.rs`

---

## Task 13b — Observable Voucher/TopUp Transport Outcomes

Depends on Task 6 (Problem Details) and Task 13 (HEAD/POST transport).

The reference client should not treat voucher/topUp updates as best-effort
fire-and-forget writes; it must surface acceptance/rejection outcomes and
react predictably.

1. Capture HTTP status and response body for voucher/topUp submissions,
   even when sent from background tasks.
2. Parse Problem Details from non-2xx responses and route recovery behavior:
   retry with fresh challenge for `challenge-not-found`, reopen/new-channel
   fallback for `channel-not-found`/`channel-finalized`, terminal error for
   signature/signer failures.
3. Parse and persist `Payment-Receipt` from successful voucher/topUp responses
   when provided. Persistence MUST be monotonic — never decrease local
   `cumulative_amount` from a late or out-of-order background response.
4. **State-update path for background tasks:** background voucher/topUp
   responses must be able to update the shared `ChannelContext` or
   persisted record. Use a channel/callback mechanism (e.g.,
   `tokio::sync::mpsc`) or persist directly with monotonic guards.
   Document the chosen approach in the implementation.
5. Expose structured debug logs/metrics for voucher acceptance latency,
   rejection reason, and fallback path taken.

### Files affected
- `crates/tempo-request/src/payment/session/streaming.rs`
- `crates/tempo-request/src/payment/session/voucher.rs`
- `crates/tempo-common/src/payment/classify.rs`

---

## Task 14 — Test fixtures + existing test updates

Depends on Tasks 1–4 (all renames must be complete).

### Fixture renames
- `seed_local_session()` → `seed_local_channel()`
- `corrupt_local_session_deposit()` → `corrupt_local_channel_deposit()`
- Update schema in fixture to match new `channels` table

### Test function renames
- `test_save_session_*` → `test_save_channel_*`
- `test_load_session_*` → `test_load_channel_*`
- `test_delete_session*` → `test_delete_channel*`
- `test_session_key_*` → removed (no more session_key)
- `test_session_status_*` → `test_channel_status_*`
- `test_concurrent_writers_see_session_*` → `test_concurrent_writers_see_channel_*`

### Removals
- Remove `test_table_needs_reset_*` tests (legacy schema detection removed)
- Replace `test_save_session_overwrites_same_origin` with test verifying
  multiple channels per origin are supported

### `make_record()` updates in flow.rs, cooperative.rs, list.rs
- `recipient` field → `payee` field
- `payer` value from DID string to raw address
- `currency` → `token`

### Files affected
- `crates/tempo-test/src/fixture.rs`
- `crates/tempo-wallet/tests/commands.rs`
- `crates/tempo-wallet/tests/structured.rs`
- `crates/tempo-common/src/payment/session/store/storage.rs`
- `crates/tempo-common/src/payment/session/store/model.rs`
- `crates/tempo-request/src/payment/session/flow.rs`
- `crates/tempo-common/src/payment/session/close/cooperative.rs`
- `crates/tempo-wallet/src/commands/sessions/list.rs`

---

## Task 15 — Doc comments, ARCHITECTURE.md, deviation docs

Depends on Tasks 1–4 (terminology must be settled).

### Module doc comments

Keep module path `payment/session/` as-is. Update doc comments:
- `store/` → "Channel persistence (SQLite)"
- `close/` → "Channel close operations"

### ARCHITECTURE.md updates
- `session/` comment → "Channel persistence (SQLite)"
- `SessionPersistenceSource` → `ChannelPersistence*`
- "Sessions persist" → "Channel state persists"
- `sessions.db` → `channels.db`
- "Keyed by origin URL" → "Keyed by channel_id, indexed by origin"
- `SessionRecord` → `ChannelRecord`
- Remove "24-hour TTL on sessions" (verified: no TTL logic exists in
  codebase; claim is stale and contradicts spec §5.2 "Channels have no
  expiry")

### Document `initial_cumulative = amount` deviation (§8.3.1)

Add doc comment in `flow.rs`:
```rust
// Deviation from spec §8.3.1: we use `amount` instead of `0` as the
// initial cumulative to pre-authorize one unit of service, avoiding
// an extra voucher round trip before the server begins delivery.
// The spec says "typically '0'" (not MUST), and server verification
// only requires cumulativeAmount >= channel.settled.
```

### Document close timing guidance (§12.3)

Record the client behavior for payer-initiated close timing:
- Spec guidance: clients SHOULD wait at least 16 minutes after
  `requestClose()` before calling `withdraw()`.
- **Implementation decision required:** either implement the 16-minute
  client-side cushion (add 60s buffer over the 15-minute contract grace)
  or rely on the contract's `closeRequestedAt + CLOSE_GRACE_PERIOD`
  exactly. If relying on contract grace only, add an explicit
  intentional-deviation entry in the Task 15c compliance matrix.
- Document default policy and any strict reference mode override.

### Document voucher transport guidance (§12.5)

Record client transport behavior for voucher updates:
- Prefer HTTP/2 multiplexing when available.
- Otherwise use separate connections/requests for voucher updates and
  content streaming.
- Keep HEAD-first voucher updates with POST fallback for compatibility.
- Add a short note in architecture docs describing this behavior as the
  implementation of the spec's `SHOULD` guidance.

### Files affected
- `crates/tempo-common/src/payment/session/mod.rs`
- `crates/tempo-common/src/payment/session/store/mod.rs`
- `crates/tempo-common/src/payment/session/store/storage.rs`
- `crates/tempo-request/src/payment/session/flow.rs`
- `crates/tempo-common/src/payment/session/close/onchain.rs`
- `ARCHITECTURE.md`

---

## Task 15b — Receipt Strictness Policy (Reference Client)

Runtime policy is already decided in the Decisions section above:
missing `Payment-Receipt` is warning-only by default.

This task narrows to conformance controls for reference usage.

1. Add a strict mode flag/config for tests
   and conformance runs that fails on missing receipts.
2. Document the default policy + strict mode behavior in `ARCHITECTURE.md`.
3. Add integration tests that cover both default behavior and strict mode.

### Files affected
- `crates/tempo-request/src/payment/session/flow.rs`
- `crates/tempo-request/src/payment/session/streaming.rs`
- `crates/tempo-request/src/args.rs`
- `ARCHITECTURE.md`

---

## Task 15c — Dependency Guarantees (`mpp` crate)

This implementation relies on protocol-critical behavior in `mpp`. Add an
explicit verification task so compliance does not depend on implicit upstream
assumptions.

### Completion status

Completed with explicit boundary tests and documentation.

### Compliance matrix (Task 15c)

| Invariant at `mpp` boundary | Why it matters | Local verification |
|---|---|---|
| EIP-712 voucher signatures are bound to `{chain_id, verifying_contract}` domain inputs | Prevents replay/acceptance across chain or escrow contexts | `crates/tempo-request/tests/mpp_boundary.rs::voucher_signature_is_bound_to_eip712_domain_inputs` verifies success on exact domain and failure when either field changes |
| Signature boundary supports 65-byte and compact ERC-2098 forms | Ensures interop with mixed signer output forms while preserving signature correctness | `crates/tempo-request/tests/mpp_boundary.rs::voucher_signature_accepts_65_byte_and_compact_erc2098_with_local_normalization` verifies canonical 65-byte acceptance and local normalization of compact signatures before verifier call |
| Unknown fields are tolerated for session request, credential payload, and receipt parsing | Preserves forward compatibility with server-side additive fields | `crates/tempo-request/tests/mpp_boundary.rs::mpp_parsing_tolerates_unknown_fields_for_session_boundary_types` verifies parse/decode succeeds with additive unknown fields |
| RFC 9457 extension fields are preserved in local Problem Details parsing | Keeps recovery-relevant extension data available without brittle schema coupling | `crates/tempo-common/src/payment/classify.rs::test_parse_problem_details_preserves_extension_fields` verifies extension passthrough |

`mpp` currently verifies canonical 65-byte voucher signatures directly. Compact
ERC-2098 signatures are normalized to canonical 65-byte form at the local
boundary before verification where that verifier path is used.

### Files affected
- `crates/tempo-request/src/payment/session/voucher.rs`
- `crates/tempo-request/src/payment/session/flow.rs`
- `crates/tempo-common/src/payment/classify.rs`
- `crates/tempo-request/tests/` (new coverage)
- `Cargo.toml` / `Cargo.lock` (if `mpp` version pin/update is needed)

---

## Task 15d — Scope Boundary: Client vs Server MUSTs

Add a short section documenting that this repository is a client/reference
wallet implementation, and explicitly mark server-only requirements from
the spec as out of scope for this codebase (while still linking them for
operators implementing servers).

Examples to list as server-side out of scope here:
- Voucher rate limiting and anti-DoS policy.
- Challenge-to-voucher mapping for audit trails.
- Receipt generation guarantees on successful paid responses.
- Per-session accounting persistence semantics on the server.

### Files affected
- `SPEC_ALIGNMENT.md`
- `ARCHITECTURE.md`

---

## Task 15e — Reference Mode Conformance Gate

Removed from scope for this alignment pass by explicit direction.
`--strict-receipts` (Task 15b) remains the conformance control shipped in this
repository for receipt strictness.

---

## Task 16 — `make check`

Run `make check` and fix any remaining issues.

---

## Integration test scenarios (future)

Tests to add after the spec alignment implementation is complete. These
cover the full channel lifecycle, persistence, reuse logic, and spec
compliance that the current test suite does not exercise.

### Channel persistence (unit — storage.rs)

1. **Save and load by channel_id** — save a `ChannelRecord`, load it
   by `channel_id`, verify all fields round-trip correctly.
2. **Multiple channels per origin** — save two records with the same
   origin but different `channel_id`, verify both are returned by
   `list_channels()` and each can be loaded independently.
3. **`find_reusable_channel` returns matching active channel** — seed
   two channels for the same origin (one active, one closing), verify
   only the active one is returned.
4. **`find_reusable_channel` returns `None` when params differ** —
   seed a channel, query with a different `payee` or `token`,
   verify `None`.
5. **`find_reusable_channel` returns most recently used** — seed two
   active channels for the same origin with identical params but
   different `last_used_at`, verify the most recent is returned.
6. **Delete by channel_id** — save a record, delete by `channel_id`,
   verify load returns `None`.
7. **Update close state** — save a record, call
   `update_channel_close_state()`, verify `state`, `close_requested_at`,
   and `grace_ready_at` are updated.
8. ~~**Schema migration from sessions.db**~~ — removed; `channels.db`
    is a fresh file with no legacy migration needed.
9. **Payer stored as raw address, not DID** — save a record with
   `payer = "0x..."`, verify the raw address persists (not a DID).
10. **Column rename: payee not recipient** — save a record, verify the
    SQL column is named `payee` (query `PRAGMA table_info`).

### Channel reuse logic (unit — flow.rs)

11. **`is_channel_reusable` matches on identity fields** — verify
    match when `payer`, `escrow`, `token`, `payee`, `chain_id` all
    match, and rejection when any one differs.
12. **`is_channel_reusable` rejects non-active state** — verify a
    record with `state = Closing` is not reusable.
13. **Reuse check uses raw address for payer comparison** — verify
    that the reuse check compares raw addresses (not DIDs).

### DID derivation (unit — persist.rs, cooperative.rs)

14. **Persist stores raw address in payer field** — call
    `persist_channel`, inspect the saved record, verify `payer` is a
    raw address (no `did:pkh:` prefix).
15. **Cooperative close derives DID from stored address** — mock a
    server, call `try_server_close` with a record whose `payer` is a
    raw address, verify the `Authorization` header contains a
    credential with `source = "did:pkh:eip155:{chainId}:{address}"`.
16. **Voucher credential derives DID correctly** — verify
    `build_voucher_credential` produces a credential whose `source`
    field is a DID derived from the signer address and chain ID.

### CLI session commands (integration — assert_cmd)

17. **`sessions list` with seeded channel shows correct JSON shape** —
    seed a channel using the new `channels` table schema, run
    `sessions list -j`, verify JSON has `"sessions"` key (product
    term), each item has `channel_id`, `origin`, `deposit`, `spent`,
    `status`.
18. **`sessions list` empty DB** — run `sessions list -j`, verify
    `{"sessions": [], "total": 0}`.
19. **`sessions list --state all` includes orphaned channels** — seed
    a local channel plus mock an on-chain channel without a local
    record, verify the orphaned channel appears with `--state all`.
20. **`sessions list` malformed row emits degraded event** — seed a
    channel, corrupt the deposit to a non-number, run `sessions list`,
    verify the `channel store degraded` analytics event fires with
    `malformed_list_drops >= 1`.
21. **`sessions close` with channel_id** — seed a channel, verify
    `sessions close 0x<channel_id>` attempts on-chain close (mock RPC
    to return channel state).
22. **`sessions close --all`** — seed multiple channels, verify all
    are closed.
23. **`sessions sync` reconciles on-chain state** — seed a local
    active channel, mock the on-chain state as `closeRequestedAt > 0`,
    run `sessions sync`, verify the local record transitions to
    `Closing` with correct `grace_ready_at`.

### Request-time flow (integration — mock 402 server)

24. **Full session open flow** — stand up a mock server that returns
    402 with a session challenge, then 200 on authorized retry. Mock
    RPC. Run `tempo-request <url>`. Verify:
    - Channel opened on-chain (mock RPC received `sendRawTransaction`)
    - `channels.db` created with a record keyed by `channel_id`
    - Record has `payee` (not `recipient`), `payer` as raw address
    - Response body returned to stdout
25. **Session reuse on second request** — after test #24, run the
    same request again. Verify:
    - No new on-chain transaction (reuse path taken)
    - `cumulative_amount` increased in persisted record
    - Debug log says "Reusing channel" (not "Reusing session")
26. **No reuse when payee differs** — after test #24, run a request
    to the same origin but with a different `recipient` in the
    challenge. Verify a new channel is opened (two records in DB).
27. **No reuse when channel is Closing** — seed a channel with
    `state = Closing`, run a request to the same origin. Verify
    `find_reusable_channel` returns `None` and a new channel opens.
28. **Dry run prints session parameters** — run `tempo-request
    --dry-run <url>` against a 402 mock. Verify no on-chain tx, no
    DB write, output includes cost and currency info.
29. **Error preserves channel state** — mock server to return 402
    (session challenge), then 500 on the authorized request. Verify
    the channel record is persisted (for on-chain dispute) and the
    error message says "channel state preserved".

### SSE streaming (integration — mock SSE server)

30. **Streaming with mid-stream voucher top-up** — mock server sends
    SSE data, then a `payment-need-voucher` event with
    `requiredCumulative` within deposit. Verify the client sends a
    voucher update request (HEAD preferred, POST fallback), streaming resumes, and `cumulative_amount` is
    updated in the persisted record.
31. **Streaming voucher clamped to deposit** — mock
    `payment-need-voucher` with `requiredCumulative > deposit`. Verify
    the voucher is clamped to `deposit` (until topUp is implemented).
32. **Streaming receipt event ends stream** — mock server sends a
    `payment-receipt` event. Verify the stream terminates cleanly.
33. **Streaming retry on stalled voucher** — mock server that doesn't
    resume after voucher POST. Verify the client retries up to
    `MAX_VOUCHER_RETRIES` times with exponential backoff.
34. **topUp when `requiredCumulative > deposit`** (after Task 11) — mock
    `payment-need-voucher` with `required > deposit`. Verify the
    client builds and sends an `action="topUp"` credential, updates
    the local deposit, then sends the voucher.

### Close operations (integration — mock server + mock RPC)

35. **Cooperative close sends correct credential** — mock a server,
    trigger cooperative close from a seeded record. Verify the
    `Authorization` header has `action="close"`, `channelId`,
    `cumulativeAmount`, `signature`, and `source` is a DID derived
    from the raw payer address.
36. **On-chain close: requestClose → Pending** — mock RPC to return
    `closeRequestedAt = 0` for the channel. Run `sessions close`.
    Verify `requestClose(channelId)` is called, local state transitions
    to `Closing` with `grace_ready_at` set.
37. **On-chain close: withdraw after grace** — mock RPC to return
    `closeRequestedAt` in the past (grace elapsed). Run `sessions
    close`. Verify `withdraw(channelId)` is called, outcome is
    `Closed`.
38. **On-chain close: grace period not elapsed** — mock RPC to return
    `closeRequestedAt` recently (grace not elapsed). Run `sessions
    close`. Verify no transaction, outcome is `Pending` with remaining
    seconds.

### Wire-format preservation

39. **Challenge `recipient` field preserved** — construct a challenge
    with `recipient` in the request JSON. Verify the code parses it
    correctly and does NOT rename to `payee` in wire-format parsing.
40. **Credential uses spec field names** — capture the `Authorization`
    header for open, voucher, and close credentials. Verify field
    names match spec: `action`, `channelId`, `cumulativeAmount`,
    `signature`, `transaction`.
41. **`source` field is a DID** — capture the credential for voucher
    and close actions. Verify `source` is
    `"did:pkh:eip155:{chainId}:{address}"`.

### Receipt handling

42. **Receipt parsed from open response** — mock server returns 200
    with `Payment-Receipt` header containing `acceptedCumulative`. Verify
    the persisted record's `cumulative_amount` reflects the accepted value.
43. **Missing receipt on 2xx logs warning** — mock server returns 200
    without `Payment-Receipt`. Verify a warning is logged but request
    succeeds.
44. **SSE receipt event updates persisted state** — mock SSE stream
    ending with a `payment-receipt` event containing `acceptedCumulative`.
    Verify the final persisted `cumulative_amount` uses the receipt value
    (not just the last signed voucher amount).
45. **Receipt `acceptedCumulative` used as reuse baseline** — persist a
    channel with `cumulative_amount = 500` from a receipt. Reuse it for
    a new request with `amount = 25`. Verify the initial cumulative is
    `525` (not re-derived from local heuristics).
46. **SSE initial header receipt is persisted before events** — mock an SSE
    response with `Payment-Receipt` header containing `acceptedCumulative`
    and delayed stream events. Verify persisted state is updated from the
    initial header even before final `payment-receipt` event.

### Voucher transport behavior

47. **HEAD-first voucher update with POST fallback** — mock server returns
    `405` for voucher `HEAD` requests and accepts `POST`. Verify client
    attempts `HEAD` first, falls back to `POST`, and continues streaming.

### ChainId defaulting

48. **Missing `methodDetails.chainId` defaults to 42431** — mock challenge
    without `chainId` and verify the flow resolves Moderato chain defaults
    rather than failing with missing field.

### Problem Details

49. **410 with `channel-not-found` problem type triggers retry** — mock
    server returns 410 with `application/problem+json` body containing
    `"type": ".../session/channel-not-found"`. Verify the retry logic
    activates (matching on type URI, not free text).
50. **402 with structured problem returns clean error** — mock server
    returns 402 with `"type": ".../session/insufficient-balance"`. Verify
    the error message includes the problem `detail` field.

### Strict protocol parsing

51. **Malformed `requiredCumulative` returns error** — mock SSE stream
    with a `payment-need-voucher` event where `requiredCumulative = "abc"`.
    Verify the stream terminates with an error (not silent `0`).
52. **Empty `requiredCumulative` returns error** — same as above with
    `requiredCumulative = ""`.

### Active session guard

53. **Concurrent reuse blocked** — spawn two concurrent requests to the
    same origin. Verify only one reuses the existing channel; the second
    either waits (if using origin lock) or opens a new channel (if using
    active flag).
54. **Stale active flag cleaned on startup** — set `active = true` on a
    channel record, then run a new request. Verify the stale flag is
    cleared and the channel is available for reuse.

### Concurrent access

55. **Two writers don't orphan channels** — spawn two concurrent
    `tempo-request` invocations to the same origin. Verify only one
    channel is opened (the second reuses the first's channel due to
    origin-based locking).
56. **Lock prevents double-open race** — simulate the race condition
    from the old origin-keyed schema: two workers with different
    channel params for the same origin. Verify both channels persist
    (not overwritten) since the new schema uses `channel_id` as PK.

---

## Not Changed (intentional)

Items deliberately kept as-is, with reasoning.

### CLI product terms

| Item | Reason |
|---|---|
| CLI command `sessions` | Product term — user-facing |
| CLI `SessionCommands` enum name | Maps to CLI `sessions` subcommand |
| `sync_sessions` function (sync.rs) | CLI product term |
| `list_sessions` function (list.rs) | CLI product term |
| `close_sessions` function (close.rs) | CLI product term |
| `maybe_delete_session_by_channel_id` (close.rs) | CLI product term; underlying call becomes `delete_channel` via import alias |
| `store as session_store` import (wallet/commands/sessions/) | CLI product term context; alias stays |
| `SessionResult<T>` alias (wallet/commands/sessions/util.rs) | CLI product term context |
| `SyncOriginResponse` (sync.rs) | Describes syncing by origin, not session-specific |
| `MissingSessionCloseTarget` error variant (error.rs) | Error message references "sessions" — user-facing |
| `BalanceBreakdown.session_count` / `BalanceInfo.active_sessions` (wallet/types.rs) | User-facing product term ("3 active sessions") |
| `compute_locked` doc comment (wallet/render.rs) | Product term in wallet crate; low priority |
| `ChannelView` struct name (render.rs) | Already correct |
| `"Spent"` display label | Close enough to spec `spent`; acceptable UX |
| JSON `"sessions"` key in list output | User-facing; tests reference `parsed["sessions"]` |
| `"No sessions."` / `"session(s) total"` messages | User-facing |

### Session-layer logic (correctly named)

| Item | Reason |
|---|---|
| `handle_session_request` (flow.rs) | Entry point for session-layer logic — IS session logic |
| `send_session_request` (flow.rs, private) | Sends voucher for an active session — IS session logic |
| `payment/router.rs` references (`handle_session_request`, `is_session`) | Protocol-level session intent routing matching spec `intent="session"` |
| Module path `payment/session/` | Disruptive rename, low clarity gain; IS session-layer logic |
| `open.rs` doc comments ("Session-open transaction building") | Describes session-layer logic that opens a channel |

### External types (not ours to rename)

| Item | Reason |
|---|---|
| `mpp::SessionRequest`, `TempoSessionExt` (sign.rs) | External `mpp` crate types |
| `SessionCredentialPayload::Open/Voucher/Close` | External `mpp` crate types; wire-format field names |
| `tempo wallet sign` session intent support | Intentional scope exclusion; this command remains charge-only |

### Analytics infrastructure (not payment-related)

| Item | Reason |
|---|---|
| `Analytics.session_id` (analytics.rs L128, L151) | PostHog per-invocation tracking session ID, NOT payment |
| `generate_session_id()` (analytics.rs L109) | PostHog analytics session, NOT payment |

### Wire-format preservation

| Item | Reason |
|---|---|
| `session_req.recipient` parsing (flow.rs L206) | Wire format per spec §6.1 |
| `recipient` local variable in flow.rs | Passed positionally to `build_open_calls` which already uses `payee` param name |
| `cumulative_amount` in voucher.rs | Maps to `cumulativeAmount` via mpp's serde rename |
| `session_req.currency` in challenge parsing | Matches challenge wire format §6.1; internal types renamed to `token` |
| `cumulative_amount` field | Matches spec `cumulativeAmount` |
| `challenge_echo` field | Client-specific concept, not in spec |

### Spec-compliant / acceptable divergences

| Item | Reason |
|---|---|
| `minVoucherDelta` not parsed | OPTIONAL server hint (§6.2); server enforces. However, a reference client SHOULD parse and respect it to avoid repeated `delta-too-small` rejections (§10.5). Consider adding to a future task if servers begin setting this field. |
| `close` credential format | Already spec-compliant (§8.3.4) — `channelId`, `cumulativeAmount`, `signature` |
| `session_credential` local var (flow.rs L444) | Local variable for open credential; clear in context |
| `send_session_request` header forwarding (flow.rs L85) | User headers forwarded via `reqwest::Client::default_headers`; not a bug |
