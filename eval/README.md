# Presto Skill Eval

Benchmarks how well AI agents understand and use presto based on its SKILL.md.

## How it works

1. **Stream-JSON parsing** — runs agents with `--stream-json` output and parses structured `tool_use` events to see exactly what Bash commands were executed
2. **Test cases** — prompts with expected behavior (should/shouldn't use presto, correct flags/URLs)
3. **Validation** — checks Bash tool calls for presto/curl invocations and validates arguments
4. **Reporting** — trigger accuracy, usage correctness, breakdown by category

## Quick start

```bash
# Run all cases with amp (5 concurrent by default)
./eval/run.sh

# Run with claude
./eval/run.sh --agent claude

# Run a single case
./eval/run.sh --case llm-ask-gpt

# Filter by category
./eval/run.sh --category trigger-positive

# Control parallelism
./eval/run.sh --parallel 8      # 8 concurrent cases
./eval/run.sh -j 10             # short form
./eval/run.sh --sequential      # one at a time (with live reports)

# A/B test a SKILL.md variant
./eval/run.sh --skill eval/variants/v2.md

# Preview without running
./eval/run.sh --dry-run
```

## What it measures

| Metric | Description |
|--------|-------------|
| **Trigger accuracy** | Does the agent correctly decide to use presto? (true positives + true negatives) |
| **Usage correctness** | When presto is used, are the flags/URL/body correct? |
| **SKILL.md quality** | Compare scores across SKILL.md variants (A/B testing) |
| **Avg duration** | Mean wall-clock time per case (seconds) |
| **Avg turns** | Mean agent turns per case (fewer = more efficient) |

## A/B testing SKILL.md variants

Test alternate versions of SKILL.md to measure which teaches agents best:

```bash
# Run baseline
./eval/run.sh --agent amp

# Run variant
./eval/run.sh --agent amp --skill eval/variants/compact.md

# Compare reports
diff eval/reports/amp.md eval/reports/amp-compact.md
```

The `--skill` flag temporarily swaps the SKILL.md in all known locations (`.ai/skills/presto/` and `~/.claude/skills/presto/`), runs the eval, then restores the originals. The variant file is saved in the run directory for reference. Reports are named `<agent>-<variant>.md`.

## Test case categories

- `trigger-positive` — prompts where the agent SHOULD use presto (LLM calls, API requests, wallet commands)
- `trigger-negative` — prompts where the agent should NOT use presto (file reads, git ops, local tasks)

### Case subcategories

| Prefix | Tests | Examples |
|--------|-------|----------|
| `llm-*` | LLM API calls via Tempo payment proxies | GPT, Claude, OpenRouter |
| `api-*` | Generic HTTP usage and flag correctness | POST JSON, verbose, save output |
| `wallet-*` | Wallet management commands | balance, whoami, login |
| `session-*` | Payment session management | list, close |
| `usage-*` | Advanced flag/option usage | custom headers, quiet mode, combined flags |
| `ambig-*` | Ambiguous prompts requiring reasoning | implicit LLM needs, "curl" mentions, "presto" the word |
| `neg-*` | Clear negative cases | file reads, git, math, local servers |

## Output

Each run creates `eval/runs/<timestamp>-<agent>/` with:
- `results.jsonl` — per-case pass/fail with reasons, duration, and turns
- `summary.json` — aggregate metrics (accuracy, avg duration, avg turns)
- `report.md` — human-readable report with per-case performance
- `SKILL.md` — the variant used (only when `--skill` is provided)
- `<case_id>/transcript.jsonl` — raw stream-json transcript
- `<case_id>/transcript.md` — human-readable transcript

## Adding test cases

Edit `eval/cases/cases.json`. Each case has:

```json
{
  "id": "unique-id",
  "category": "trigger-positive|trigger-negative",
  "prompt": "What to ask the agent",
  "expect": {
    "presto": {
      "should_invoke": true,
      "url_pattern": "regex for URL",
      "method": "POST",
      "has_flag": "--json",
      "argv_contains": ["--dry-run"],
      "json_checks": [".messages | type == \"array\""]
    },
    "curl": { "should_invoke": false }
  }
}
```
