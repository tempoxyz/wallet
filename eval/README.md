#  Tempo WalletSkill Eval

Benchmarks how well AI agents understand and use  tempo-walletbased on its SKILL.md.

Powered by [promptfoo](https://promptfoo.dev).

## How it works

1. **Custom provider** — runs agents (`amp`, `claude`) in isolated temp directories with `--stream-json` output
2. **Test cases** — prompts with expected behavior (should/shouldn't use presto, correct flags/URLs)
3. **Assertions** — JavaScript functions parse Bash tool calls for presto/curl invocations and validate arguments
4. **Reporting** — trigger accuracy, usage correctness, breakdown by category via promptfoo's UI

## Quick start

```bash
cd eval

# Run all cases with both agents
promptfoo eval --no-cache

# Run with a single agent
promptfoo eval --no-cache --filter-providers amp
promptfoo eval --no-cache --filter-providers claude

# Run a single case
promptfoo eval --no-cache --filter-pattern llm-ask-gpt

# Filter by category
promptfoo eval --no-cache --filter-pattern "trigger-positive"

# Control parallelism
promptfoo eval --no-cache -j 8

# Run each case multiple times (measure flakiness)
promptfoo eval --no-cache --repeat 3

# View results in web UI
promptfoo view
```

## What it measures

| Metric | Description |
|--------|-------------|
| **Trigger accuracy** | Does the agent correctly decide to use presto? (true positives + true negatives) |
| **Usage correctness** | When  tempo-walletis used, are the flags/URL/body correct? |
| **Overall** | Combined trigger + usage score |

## Architecture

```
eval/
├── promptfooconfig.yaml  # Config, providers, assertions, and all test cases
├── provider.js           # Custom promptfoo provider (runs amp/claude in temp dirs)
├── assertions.js         # Assertion logic (parses stream-json for presto/curl)
└── README.md
```

## Test case categories

- `trigger-positive` — prompts where the agent SHOULD use presto
- `trigger-negative` — prompts where the agent should NOT use presto

### Case subcategories

| Prefix | Tests | Examples |
|--------|-------|----------|
| `llm-*` | LLM API calls via Tempo payment proxies | GPT, Claude, OpenRouter, DALL-E, embeddings, Whisper |
| `api-*` | Generic HTTP usage and flag correctness | POST JSON, verbose, save output |
| `wallet-*` | Wallet management commands | balance, whoami, login |
| `session-*` | Payment session management | list (state filters), info, close, recover |
| `usage-*` | Advanced flag/option usage | custom headers, quiet mode, timeout, combined flags |
| `ambig-*` | Ambiguous prompts requiring reasoning | implicit LLM needs, "curl" mentions, free vs paid APIs |
| `neg-*` | Clear negative cases | file reads, git, math, local servers, has API key |
| `real-*` | Real-world scenarios | Firecrawl, Exa, ElevenLabs, model comparison, translate |

## Adding test cases

Add entries to the `tests` array in `promptfooconfig.yaml`:

```yaml
- description: "my-case [trigger-positive]"
  vars:
    prompt: "What to ask the agent"
    expect: '{"presto":{"should_invoke":true,"url_pattern":"...","method":"POST"}}'
```

