#  Tempo WalletSkill Eval Report

## Summary

| Metric | Value |
|--------|-------|
| Total cases | 39 |
| Passed | 39 |
| Failed | 0 |
| Trigger accuracy | 100% |
| Usage accuracy | 100% |
| Avg duration | 12.4s |
| Avg turns | 2.5 |
| Wall time | 2m11s |

## Results by Category

| Category | Passed | Total | Rate |
|----------|--------|-------|------|
| trigger-negative | 15 | 15 | 100% |
| trigger-positive | 24 | 24 | 100% |

## All Cases

| Case | Category | Trigger | Usage | Result | Duration | Turns | Notes |
|------|----------|---------|-------|--------|----------|-------|-------|
| llm-ask-gpt | trigger-positive | ✅ | ✅ | ✅ PASS | 15.6s | 3 |  |
| llm-ask-claude | trigger-positive | ✅ | ✅ | ✅ PASS | 23.1s | 4 |  |
| llm-openrouter | trigger-positive | ✅ | ✅ | ✅ PASS | 17.1s | 3 |  |
| llm-generic-no-key | trigger-positive | ✅ | ✅ | ✅ PASS | 12.9s | 2 |  |
| llm-with-spending-limit | trigger-positive | ✅ | ✅ | ✅ PASS | 16.5s | 3 |  |
| llm-dry-run | trigger-positive | ✅ | ✅ | ✅ PASS | 13.9s | 3 |  |
| api-generic-http | trigger-positive | ✅ | ✅ | ✅ PASS | 12.1s | 3 |  |
| wallet-check-balance | trigger-positive | ✅ | ✅ | ✅ PASS | 6.8s | 2 |  |
| wallet-whoami | trigger-positive | ✅ | ✅ | ✅ PASS | 17.1s | 4 |  |
| wallet-login | trigger-positive | ✅ | ✅ | ✅ PASS | 12.6s | 3 |  |
| session-list | trigger-positive | ✅ | ✅ | ✅ PASS | 10.0s | 2 |  |
| api-post-json | trigger-positive | ✅ | ✅ | ✅ PASS | 9.0s | 3 |  |
| api-verbose | trigger-positive | ✅ | ✅ | ✅ PASS | 9.8s | 3 |  |
| api-save-output | trigger-positive | ✅ | ✅ | ✅ PASS | 11.9s | 3 |  |
| api-services-directory | trigger-positive | ✅ | ✅ | ✅ PASS | 19.4s | 3 |  |
| neg-read-file | trigger-negative | ✅ | ✅ | ✅ PASS | 9.8s | 2 |  |
| neg-git-status | trigger-negative | ✅ | ✅ | ✅ PASS | 8.2s | 2 |  |
| neg-write-code | trigger-negative | ✅ | ✅ | ✅ PASS | 4.0s | 1 |  |
| neg-local-build | trigger-negative | ✅ | ✅ | ✅ PASS | 10.9s | 3 |  |
| neg-grep-code | trigger-negative | ✅ | ✅ | ✅ PASS | 5.1s | 2 |  |
| neg-explain-code | trigger-negative | ✅ | ✅ | ✅ PASS | 13.3s | 2 |  |
| neg-math | trigger-negative | ✅ | ✅ | ✅ PASS | 1.9s | 1 |  |
| neg-edit-file | trigger-negative | ✅ | ✅ | ✅ PASS | 5.6s | 2 |  |
| neg-web-search | trigger-negative | ✅ | ✅ | ✅ PASS | 15.7s | 2 |  |
| neg-local-server | trigger-negative | ✅ | ✅ | ✅ PASS | 8.0s | 2 |  |
| ambig-implicit-llm | trigger-positive | ✅ | ✅ | ✅ PASS | 13.0s | 2 |  |
| ambig-translate | trigger-positive | ✅ | ✅ | ✅ PASS | 8.2s | 2 |  |
| ambig-summarize-url | trigger-negative | ✅ | ✅ | ✅ PASS | 31.2s | 2 |  |
| ambig-public-api | trigger-negative | ✅ | ✅ | ✅ PASS | 12.5s | 2 |  |
| ambig-presto-the-word | trigger-negative | ✅ | ✅ | ✅ PASS | 7.0s | 2 |  |
| ambig-curl-explicit | trigger-positive | ✅ | ✅ | ✅ PASS | 10.1s | 2 |  |
| usage-custom-headers | trigger-positive | ✅ | ✅ | ✅ PASS | 11.6s | 3 |  |
| usage-quiet-mode | trigger-positive | ✅ | ✅ | ✅ PASS | 11.5s | 3 |  |
| usage-network-filter | trigger-positive | ✅ | ✅ | ✅ PASS | 8.8s | 2 |  |
| usage-combined-flags | trigger-positive | ✅ | ✅ | ✅ PASS | 12.6s | 3 |  |
| usage-include-headers | trigger-positive | ✅ | ✅ | ✅ PASS | 10.2s | 3 |  |
| usage-session-close | trigger-positive | ✅ | ✅ | ✅ PASS | 28.6s | 3 |  |
| neg-github-api | trigger-negative | ✅ | ✅ | ✅ PASS | 20.3s | 6 |  |
| neg-internal-api | trigger-negative | ✅ | ✅ | ✅ PASS | 8.7s | 2 |  |
