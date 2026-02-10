# FOLLOWUP.md — Ambiguous Items Needing Clarification

- [ ] Trim `tempoctl q --help` — too many options for what should feel simple. Reduce visible surface area.

## `logout` — clarify or remove?

Feedback said "clarify or remove logout." Questions:
- What does `logout` currently do? Just clear local credentials/keystore reference?
- Should we keep it but rename (e.g., `disconnect`)?
- Or remove entirely and rely on `whoami switch` / `whoami delete`?

## `services` — aliases unclear

Feedback: "tempoctl services — aliases unclear." Questions:
- Which aliases are confusing? The subcommand name itself, or the `list`/`info` subcommands?
- Should `services` be renamed to something else (e.g., `providers`, `endpoints`)?
- Or should it be folded into another command?

## Minimal surface area — what specifically to cut?

"Minimal surface area — too extensive for a simple tool." Beyond the explicit removals above:
- Which commands feel extraneous? Candidates: `inspect`, `networks`, `balance` (already in `whoami`)?
- Should some commands become hidden/advanced rather than removed?

## `tempoctl q --help` — which options to trim?

Current options: `--max-amount`, `--confirm`, `--dry-run`, `--no-swap`, `--network`, `--insecure`, `--include`, `--head`, `--output-format`, `--output`, `--verbosity`, `--color`, `--quiet`, `--json-output`, `-X`, `-H`, `-A`, `-L`, `--connect-timeout`, `--max-time`, `-d`, `--json`, `--rpc`.
- Which of these should be removed vs hidden vs kept?
- Should global display options (verbosity, color, quiet, json-output) be hidden from subcommand help?

## From Ergonomics Review

### #7: `inspect` command broken on POST endpoints

`inspect` only sends GET and checks for 402, but payment endpoints are POST-only. Options:
- Support `-X` method flag on `inspect`
- Use the service directory to show payment info without making a request
- Remove `inspect` entirely (it's part of the "minimal surface area" discussion above)

### #13: `config` shows "No payment methods" even when wallet is connected

Config and wallet state stored separately. Options:
- Merge wallet state display into whatever replaces `config`
- Show wallet info in `whoami` (may already be handled by the whoami/keys merge in PLAN.md)

### #14: `balance` shows warnings for all networks including unreachable ones

Options:
- Only query networks the user has configured/used
- Default to the user's active network, require `-n all` to see everything
- Suppress warnings for networks with no configured RPC

### #19: `--json-output` vs `--output-format` overlap

These need to be unified, but the design question is: one global `--output-format` flag, or per-subcommand? And should we keep `--json-output` as a shorthand for `--output-format json`?

### #32: Inconsistent exit codes

No documented exit code contract. Need to decide on a scheme:
- 0 = success
- 1 = general error
- 2 = usage/CLI error
- 3 = config/auth error
- 4 = network error
- What about HTTP errors? Separate code or use `--fail`?

### #37: `config --output-format json` shows `evm: null` when wallet is active

Related to #13. The programmatic API for inspecting full tool state is broken. Depends on how we resolve the config/wallet split.
