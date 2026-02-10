# DONOTDO.md — Out of Scope

Items from ERGONOMICS.md that are outside the scope of tempoctl fixes (server-side issues, large features, or not actionable).

## Server-Side / Not Our Bug

- **#2**: Services charge on upstream failure (Exa, Twitter, Anthropic) — This is a server-side payment gateway issue. tempoctl can't fix upstream services accepting payment then failing. The suggestion to "validate required headers before paying" adds fragile service-specific logic. Refunds are server-side.
- **#33**: PostHog telemetry fires 3 concurrent connections — This is a PostHog SDK behavior, not a tempoctl design choice.

## Large Features / Wrong Scope

- **#24**: Transaction history — Requires indexer/API infrastructure, not a CLI fix.
- **#25**: Spending summary — Same as above, needs backend support.
- **#27**: Service-name shorthand for `query` — Nice idea but significant design work (URL construction from partial paths, ambiguity with regular URLs). Revisit later.
- **#28**: `wallet` subcommand is nearly empty — Reorganizing command hierarchy is a separate design project.
- **#29**: Man pages — Low priority, not a bug.
- **#23**: `config set` command — Config was removed per PLAN.md; moot.

## Not Worth Changing

- **#16**: `-d` sends POST without warning — This is standard curl behavior. We're curl-compatible here, not wget-compatible.
- **#18**: `-v` means verbosity not headers — Already decided: we follow clap conventions for `-v` as verbosity. `-i`/`-I` for headers matches curl.
- **#34**: `whoami` defaults to testnet — This is correct behavior when the user is on testnet. No action needed.
- **#35**: Redirects not followed by default — We follow curl conventions (`-L` to follow). Despite being "wget-like" in description, curl behavior is the right default for a payment tool (auto-following redirects could lead to paying the wrong endpoint).
- **#20**: Spending limit warning text — Informational message, not blocking. Can revisit if users complain.
- **#21**: `--no-swap` documentation — This is a help-text improvement at best; the swap mechanism is documented in Tempo docs, not tempoctl's job to explain DeFi mechanics.
