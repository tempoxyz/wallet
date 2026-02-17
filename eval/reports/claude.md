# Presto Skill Eval Report

## Summary

| Metric | Value |
|--------|-------|
| Total cases | 39 |
| Passed | 39 |
| Failed | 0 |
| Trigger accuracy | 100% |
| Usage accuracy | 100% |
| Avg duration | 17.9s |
| Avg turns | 4.3 |
| Wall time | 2m53s |

## Results by Category

| Category | Passed | Total | Rate |
|----------|--------|-------|------|
| trigger-negative | 15 | 15 | 100% |
| trigger-positive | 24 | 24 | 100% |

## All Cases

| Case | Category | Trigger | Usage | Result | Duration | Turns | Notes |
|------|----------|---------|-------|--------|----------|-------|-------|
| llm-ask-gpt | trigger-positive | ✅ | ✅ | ✅ PASS | 16.6s | 4 |  |
| llm-ask-claude | trigger-positive | ✅ | ✅ | ✅ PASS | 101.3s | 19 |  |
| llm-openrouter | trigger-positive | ✅ | ✅ | ✅ PASS | 19.1s | 5 |  |
| llm-generic-no-key | trigger-positive | ✅ | ✅ | ✅ PASS | 33.2s | 7 |  |
| llm-with-spending-limit | trigger-positive | ✅ | ✅ | ✅ PASS | 38.9s | 6 |  |
| llm-dry-run | trigger-positive | ✅ | ✅ | ✅ PASS | 29.3s | 5 |  |
| api-generic-http | trigger-positive | ✅ | ✅ | ✅ PASS | 13.6s | 4 |  |
| wallet-check-balance | trigger-positive | ✅ | ✅ | ✅ PASS | 11.5s | 5 |  |
| wallet-whoami | trigger-positive | ✅ | ✅ | ✅ PASS | 16.4s | 6 |  |
| wallet-login | trigger-positive | ✅ | ✅ | ✅ PASS | 12.1s | 4 |  |
| session-list | trigger-positive | ✅ | ✅ | ✅ PASS | 7.7s | 2 |  |
| api-post-json | trigger-positive | ✅ | ✅ | ✅ PASS | 14.9s | 5 |  |
| api-verbose | trigger-positive | ✅ | ✅ | ✅ PASS | 12.7s | 4 |  |
| api-save-output | trigger-positive | ✅ | ✅ | ✅ PASS | 9.6s | 4 |  |
| api-services-directory | trigger-positive | ✅ | ✅ | ✅ PASS | 23.8s | 4 |  |
| neg-read-file | trigger-negative | ✅ | ✅ | ✅ PASS | 12.2s | 2 |  |
| neg-git-status | trigger-negative | ✅ | ✅ | ✅ PASS | 6.0s | 2 |  |
| neg-write-code | trigger-negative | ✅ | ✅ | ✅ PASS | 3.7s | 1 |  |
| neg-local-build | trigger-negative | ✅ | ✅ | ✅ PASS | 10.3s | 3 |  |
| neg-grep-code | trigger-negative | ✅ | ✅ | ✅ PASS | 5.3s | 2 |  |
| neg-explain-code | trigger-negative | ✅ | ✅ | ✅ PASS | 22.1s | 4 |  |
| neg-math | trigger-negative | ✅ | ✅ | ✅ PASS | 1.6s | 1 |  |
| neg-edit-file | trigger-negative | ✅ | ✅ | ✅ PASS | 4.7s | 2 |  |
| neg-web-search | trigger-negative | ✅ | ✅ | ✅ PASS | 18.1s | 2 |  |
| neg-local-server | trigger-negative | ✅ | ✅ | ✅ PASS | 8.4s | 2 |  |
| ambig-implicit-llm | trigger-positive | ✅ | ✅ | ✅ PASS | 14.4s | 4 |  |
| ambig-translate | trigger-positive | ✅ | ✅ | ✅ PASS | 17.6s | 5 |  |
| ambig-summarize-url | trigger-negative | ✅ | ✅ | ✅ PASS | 29.3s | 3 |  |
| ambig-public-api | trigger-negative | ✅ | ✅ | ✅ PASS | 12.8s | 4 |  |
| ambig-presto-the-word | trigger-negative | ✅ | ✅ | ✅ PASS | 15.8s | 3 |  |
| ambig-curl-explicit | trigger-positive | ✅ | ✅ | ✅ PASS | 66.8s | 15 |  |
| usage-custom-headers | trigger-positive | ✅ | ✅ | ✅ PASS | 10.6s | 4 |  |
| usage-quiet-mode | trigger-positive | ✅ | ✅ | ✅ PASS | 12.9s | 4 |  |
| usage-network-filter | trigger-positive | ✅ | ✅ | ✅ PASS | 12.1s | 5 |  |
| usage-combined-flags | trigger-positive | ✅ | ✅ | ✅ PASS | 14.7s | 4 |  |
| usage-include-headers | trigger-positive | ✅ | ✅ | ✅ PASS | 11.2s | 4 |  |
| usage-session-close | trigger-positive | ✅ | ✅ | ✅ PASS | 9.6s | 4 |  |
| neg-github-api | trigger-negative | ✅ | ✅ | ✅ PASS | 7.2s | 2 |  |
| neg-internal-api | trigger-negative | ✅ | ✅ | ✅ PASS | 9.4s | 3 |  |
