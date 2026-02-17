#  Tempo WalletSkill Eval

Benchmarks how well AI agents understand and use  tempo-walletbased on its SKILL.md.

## How it works

1. **Stream-JSON parsing** — runs agents with `--stream-json` output and parses structured `tool_use` events to see exactly what Bash commands were executed
2. **Test cases** — prompts with expected behavior (should/shouldn't use presto, correct flags/URLs)
3. **Validation** — checks Bash tool calls for presto/curl invocations and validates arguments
4. **Reporting** — trigger accuracy, usage correctness, breakdown by category

## Quick start

```bash
# Run all cases with amp
./eval/run.sh

# Run with claude
./eval/run.sh --agent claude

# Run a single case
./eval/run.sh --case llm-ask-gpt

# Filter by category
./eval/run.sh --category trigger-positive

# Preview without running
./eval/run.sh --dry-run
```

## What it measures

| Metric | Description |
|--------|-------------|
| **Trigger accuracy** | Does the agent correctly decide to use presto? (true positives + true negatives) |
| **Usage correctness** | When  tempo-walletis used, are the flags/URL/body correct? |
| **SKILL.md quality** | Compare scores across SKILL.md variants (A/B testing) |

## Test case categories

- `trigger-positive` — prompts where the agent SHOULD use  tempo-wallet(LLM calls, API requests, wallet commands)
- `trigger-negative` — prompts where the agent should NOT use  tempo-wallet(file reads, git ops, local tasks)

## Output

Each run creates `eval/runs/<timestamp>-<agent>/` with:
- `results.jsonl` — per-case pass/fail with reasons
- `summary.json` — aggregate metrics
- `report.md` — human-readable report
- `<case_id>/tool_calls.jsonl` — raw shim logs
- `<case_id>/transcript.txt` — agent output

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
