# Presto Skill Eval Report

## Summary

| Metric | Value |
|--------|-------|
| Total cases | 25 |
| Passed | 25 |
| Failed | 0 |
| Trigger accuracy | 100% |
| Usage accuracy | 100% |
| Avg duration | 10.5s |
| Avg turns | 2.4 |
| Wall time | 5m15s |

## Results by Category

| Category | Passed | Total | Rate |
|----------|--------|-------|------|
| trigger-negative | 10 | 10 | 100% |
| trigger-positive | 15 | 15 | 100% |

## All Cases

| Case | Category | Trigger | Usage | Result | Duration | Turns | Notes |
|------|----------|---------|-------|--------|----------|-------|-------|
| llm-ask-gpt | trigger-positive | ✅ | ✅ | ✅ PASS | 9.1s | 2 |  |
| llm-ask-claude | trigger-positive | ✅ | ✅ | ✅ PASS | 23.3s | 4 |  |
| llm-openrouter | trigger-positive | ✅ | ✅ | ✅ PASS | 13.1s | 3 |  |
| llm-generic-no-key | trigger-positive | ✅ | ✅ | ✅ PASS | 10.0s | 2 |  |
| llm-with-spending-limit | trigger-positive | ✅ | ✅ | ✅ PASS | 20.7s | 3 |  |
| llm-dry-run | trigger-positive | ✅ | ✅ | ✅ PASS | 10.0s | 3 |  |
| api-generic-http | trigger-positive | ✅ | ✅ | ✅ PASS | 9.6s | 3 |  |
| wallet-check-balance | trigger-positive | ✅ | ✅ | ✅ PASS | 5.5s | 2 |  |
| wallet-whoami | trigger-positive | ✅ | ✅ | ✅ PASS | 15.4s | 4 |  |
| wallet-login | trigger-positive | ✅ | ✅ | ✅ PASS | 9.8s | 3 |  |
| session-list | trigger-positive | ✅ | ✅ | ✅ PASS | 13.3s | 3 |  |
| api-post-json | trigger-positive | ✅ | ✅ | ✅ PASS | 9.0s | 3 |  |
| api-verbose | trigger-positive | ✅ | ✅ | ✅ PASS | 7.4s | 2 |  |
| api-save-output | trigger-positive | ✅ | ✅ | ✅ PASS | 10.7s | 3 |  |
| api-services-directory | trigger-positive | ✅ | ✅ | ✅ PASS | 17.6s | 3 |  |
| neg-read-file | trigger-negative | ✅ | ✅ | ✅ PASS | 7.1s | 2 |  |
| neg-git-status | trigger-negative | ✅ | ✅ | ✅ PASS | 6.8s | 2 |  |
| neg-write-code | trigger-negative | ✅ | ✅ | ✅ PASS | 3.4s | 1 |  |
| neg-local-build | trigger-negative | ✅ | ✅ | ✅ PASS | 11.2s | 2 |  |
| neg-grep-code | trigger-negative | ✅ | ✅ | ✅ PASS | 4.4s | 2 |  |
| neg-explain-code | trigger-negative | ✅ | ✅ | ✅ PASS | 12.9s | 2 |  |
| neg-math | trigger-negative | ✅ | ✅ | ✅ PASS | 1.8s | 1 |  |
| neg-edit-file | trigger-negative | ✅ | ✅ | ✅ PASS | 5.6s | 2 |  |
| neg-web-search | trigger-negative | ✅ | ✅ | ✅ PASS | 18.9s | 2 |  |
| neg-local-server | trigger-negative | ✅ | ✅ | ✅ PASS | 6.4s | 2 |  |
