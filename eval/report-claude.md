# Presto Skill Eval Report

## Summary

| Metric | Value |
|--------|-------|
| Total cases | 25 |
| Passed | 25 |
| Failed | 0 |
| Trigger accuracy | 100% |
| Usage accuracy | 100% |

## Results by Category

| Category | Passed | Total | Rate |
|----------|--------|-------|------|
| trigger-negative | 10 | 10 | 100% |
| trigger-positive | 15 | 15 | 100% |

## All Cases

| Case | Category | Trigger | Usage | Result | Notes |
|------|----------|---------|-------|--------|-------|
| llm-ask-gpt | trigger-positive | ✅ | ✅ | ✅ PASS |  |
| llm-ask-claude | trigger-positive | ✅ | ✅ | ✅ PASS |  |
| llm-openrouter | trigger-positive | ✅ | ✅ | ✅ PASS |  |
| llm-generic-no-key | trigger-positive | ✅ | ✅ | ✅ PASS |  |
| llm-with-spending-limit | trigger-positive | ✅ | ✅ | ✅ PASS |  |
| llm-dry-run | trigger-positive | ✅ | ✅ | ✅ PASS |  |
| api-generic-http | trigger-positive | ✅ | ✅ | ✅ PASS |  |
| wallet-check-balance | trigger-positive | ✅ | ✅ | ✅ PASS |  |
| wallet-whoami | trigger-positive | ✅ | ✅ | ✅ PASS |  |
| wallet-login | trigger-positive | ✅ | ✅ | ✅ PASS |  |
| session-list | trigger-positive | ✅ | ✅ | ✅ PASS |  |
| api-post-json | trigger-positive | ✅ | ✅ | ✅ PASS |  |
| api-verbose | trigger-positive | ✅ | ✅ | ✅ PASS |  |
| api-save-output | trigger-positive | ✅ | ✅ | ✅ PASS |  |
| api-services-directory | trigger-positive | ✅ | ✅ | ✅ PASS |  |
| neg-read-file | trigger-negative | ✅ | ✅ | ✅ PASS |  |
| neg-git-status | trigger-negative | ✅ | ✅ | ✅ PASS |  |
| neg-write-code | trigger-negative | ✅ | ✅ | ✅ PASS |  |
| neg-local-build | trigger-negative | ✅ | ✅ | ✅ PASS |  |
| neg-grep-code | trigger-negative | ✅ | ✅ | ✅ PASS |  |
| neg-explain-code | trigger-negative | ✅ | ✅ | ✅ PASS |  |
| neg-math | trigger-negative | ✅ | ✅ | ✅ PASS |  |
| neg-edit-file | trigger-negative | ✅ | ✅ | ✅ PASS |  |
| neg-web-search | trigger-negative | ✅ | ✅ | ✅ PASS |  |
| neg-local-server | trigger-negative | ✅ | ✅ | ✅ PASS |  |
