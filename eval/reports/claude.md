# Presto Skill Eval Report

## Summary

| Metric | Value |
|--------|-------|
| Total cases | 25 |
| Passed | 25 |
| Failed | 0 |
| Trigger accuracy | 100% |
| Usage accuracy | 100% |
| Avg duration | 17.1s |
| Avg turns | 3.8 |
| Wall time | 7m43s |

## Results by Category

| Category | Passed | Total | Rate |
|----------|--------|-------|------|
| trigger-negative | 10 | 10 | 100% |
| trigger-positive | 15 | 15 | 100% |

## All Cases

| Case | Category | Trigger | Usage | Result | Duration | Turns | Notes |
|------|----------|---------|-------|--------|----------|-------|-------|
| llm-ask-gpt | trigger-positive | ✅ | ✅ | ✅ PASS | 15.8s | 4 |  |
| llm-ask-claude | trigger-positive | ✅ | ✅ | ✅ PASS | 23.0s | 5 |  |
| llm-openrouter | trigger-positive | ✅ | ✅ | ✅ PASS | 58.5s | 13 |  |
| llm-generic-no-key | trigger-positive | ✅ | ✅ | ✅ PASS | 39.0s | 5 |  |
| llm-with-spending-limit | trigger-positive | ✅ | ✅ | ✅ PASS | 17.3s | 4 |  |
| llm-dry-run | trigger-positive | ✅ | ✅ | ✅ PASS | 22.7s | 5 |  |
| api-generic-http | trigger-positive | ✅ | ✅ | ✅ PASS | 15.2s | 4 |  |
| wallet-check-balance | trigger-positive | ✅ | ✅ | ✅ PASS | 10.6s | 4 |  |
| wallet-whoami | trigger-positive | ✅ | ✅ | ✅ PASS | 16.0s | 5 |  |
| wallet-login | trigger-positive | ✅ | ✅ | ✅ PASS | 13.5s | 4 |  |
| session-list | trigger-positive | ✅ | ✅ | ✅ PASS | 11.3s | 4 |  |
| api-post-json | trigger-positive | ✅ | ✅ | ✅ PASS | 12.4s | 4 |  |
| api-verbose | trigger-positive | ✅ | ✅ | ✅ PASS | 13.8s | 4 |  |
| api-save-output | trigger-positive | ✅ | ✅ | ✅ PASS | 15.0s | 4 |  |
| api-services-directory | trigger-positive | ✅ | ✅ | ✅ PASS | 21.6s | 4 |  |
| neg-read-file | trigger-negative | ✅ | ✅ | ✅ PASS | 12.0s | 2 |  |
| neg-git-status | trigger-negative | ✅ | ✅ | ✅ PASS | 9.4s | 2 |  |
| neg-write-code | trigger-negative | ✅ | ✅ | ✅ PASS | 4.0s | 1 |  |
| neg-local-build | trigger-negative | ✅ | ✅ | ✅ PASS | 27.2s | 6 |  |
| neg-grep-code | trigger-negative | ✅ | ✅ | ✅ PASS | 8.0s | 2 |  |
| neg-explain-code | trigger-negative | ✅ | ✅ | ✅ PASS | 21.3s | 2 |  |
| neg-math | trigger-negative | ✅ | ✅ | ✅ PASS | 2.9s | 1 |  |
| neg-edit-file | trigger-negative | ✅ | ✅ | ✅ PASS | 5.9s | 2 |  |
| neg-web-search | trigger-negative | ✅ | ✅ | ✅ PASS | 16.1s | 2 |  |
| neg-local-server | trigger-negative | ✅ | ✅ | ✅ PASS | 14.4s | 4 |  |
