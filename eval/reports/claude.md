#  Tempo WalletSkill Eval Report

## Summary

| Metric | Value |
|--------|-------|
| Total cases | 59 |
| Passed | 58 |
| Failed | 1 |
| Trigger accuracy | 98% |
| Usage accuracy | 100% |
| Avg duration | 24.9s |
| Avg turns | 4.9 |
| Wall time | 10m00s |

## Results by Category

| Category | Passed | Total | Rate |
|----------|--------|-------|------|
| trigger-negative | 23 | 24 | 95% |
| trigger-positive | 35 | 35 | 100% |

## All Cases

| Case | Category | Trigger | Usage | Result | Duration | Turns | Notes |
|------|----------|---------|-------|--------|----------|-------|-------|
| llm-ask-gpt | trigger-positive | ✅ | ✅ | ✅ PASS | 14.0s | 4 |  |
| llm-ask-claude | trigger-positive | ✅ | ✅ | ✅ PASS | 92.2s | 17 |  |
| llm-openrouter | trigger-positive | ✅ | ✅ | ✅ PASS | 16.7s | 5 |  |
| llm-generic-no-key | trigger-positive | ✅ | ✅ | ✅ PASS | 17.2s | 5 |  |
| llm-with-spending-limit | trigger-positive | ✅ | ✅ | ✅ PASS | 61.7s | 10 |  |
| llm-dry-run | trigger-positive | ✅ | ✅ | ✅ PASS | 20.3s | 5 |  |
| api-generic-http | trigger-positive | ✅ | ✅ | ✅ PASS | 11.2s | 4 |  |
| wallet-check-balance | trigger-positive | ✅ | ✅ | ✅ PASS | 10.6s | 4 |  |
| wallet-whoami | trigger-positive | ✅ | ✅ | ✅ PASS | 13.0s | 5 |  |
| wallet-login | trigger-positive | ✅ | ✅ | ✅ PASS | 11.9s | 4 |  |
| session-list | trigger-positive | ✅ | ✅ | ✅ PASS | 10.8s | 4 |  |
| api-post-json | trigger-positive | ✅ | ✅ | ✅ PASS | 9.6s | 4 |  |
| api-verbose | trigger-positive | ✅ | ✅ | ✅ PASS | 12.0s | 4 |  |
| api-save-output | trigger-positive | ✅ | ✅ | ✅ PASS | 14.7s | 4 |  |
| api-services-directory | trigger-positive | ✅ | ✅ | ✅ PASS | 19.6s | 4 |  |
| neg-read-file | trigger-negative | ✅ | ✅ | ✅ PASS | 11.3s | 2 |  |
| neg-git-status | trigger-negative | ✅ | ✅ | ✅ PASS | 6.0s | 2 |  |
| neg-write-code | trigger-negative | ✅ | ✅ | ✅ PASS | 2.8s | 1 |  |
| neg-local-build | trigger-negative | ✅ | ✅ | ✅ PASS | 27.2s | 8 |  |
| neg-grep-code | trigger-negative | ✅ | ✅ | ✅ PASS | 5.3s | 2 |  |
| neg-explain-code | trigger-negative | ✅ | ✅ | ✅ PASS | 23.7s | 4 |  |
| neg-math | trigger-negative | ✅ | ✅ | ✅ PASS | 1.2s | 1 |  |
| neg-edit-file | trigger-negative | ✅ | ✅ | ✅ PASS | 6.8s | 2 |  |
| neg-web-search | trigger-negative | ✅ | ✅ | ✅ PASS | 16.6s | 2 |  |
| neg-local-server | trigger-negative | ✅ | ✅ | ✅ PASS | 5.6s | 2 |  |
| ambig-implicit-llm | trigger-positive | ✅ | ✅ | ✅ PASS | 152.2s | 4 |  |
| ambig-translate | trigger-positive | ✅ | ✅ | ✅ PASS | 15.5s | 5 |  |
| ambig-summarize-url | trigger-negative | ✅ | ✅ | ✅ PASS | 23.8s | 3 |  |
| ambig-public-api | trigger-negative | ✅ | ✅ | ✅ PASS | 38.2s | 6 |  |
| ambig-presto-the-word | trigger-negative | ✅ | ✅ | ✅ PASS | 22.4s | 4 |  |
| ambig-curl-explicit | trigger-positive | ✅ | ✅ | ✅ PASS | 24.1s | 5 |  |
| usage-custom-headers | trigger-positive | ✅ | ✅ | ✅ PASS | 11.3s | 4 |  |
| usage-quiet-mode | trigger-positive | ✅ | ✅ | ✅ PASS | 10.0s | 4 |  |
| usage-network-filter | trigger-positive | ✅ | ✅ | ✅ PASS | 9.1s | 4 |  |
| usage-combined-flags | trigger-positive | ✅ | ✅ | ✅ PASS | 12.3s | 4 |  |
| usage-include-headers | trigger-positive | ✅ | ✅ | ✅ PASS | 13.1s | 4 |  |
| usage-session-close | trigger-positive | ✅ | ✅ | ✅ PASS | 11.8s | 4 |  |
| neg-github-api | trigger-negative | ✅ | ✅ | ✅ PASS | 5.4s | 2 |  |
| neg-internal-api | trigger-negative | ❌ | ✅ | ❌ FAIL | 16.8s | 4 |  tempo-walletinvoked but should not have been (1 calls) |
| real-scrape-docs | trigger-positive | ✅ | ✅ | ✅ PASS | 57.9s | 10 |  |
| real-web-search | trigger-positive | ✅ | ✅ | ✅ PASS | 35.9s | 7 |  |
| real-code-review | trigger-positive | ✅ | ✅ | ✅ PASS | 54.3s | 7 |  |
| real-generate-tests | trigger-positive | ✅ | ✅ | ✅ PASS | 45.3s | 5 |  |
| real-compare-models | trigger-positive | ✅ | ✅ | ✅ PASS | 78.9s | 15 |  |
| real-crawl-site | trigger-positive | ✅ | ✅ | ✅ PASS | 56.6s | 9 |  |
| real-tts-audio | trigger-positive | ✅ | ✅ | ✅ PASS | 26.6s | 6 |  |
| real-json-output | trigger-positive | ✅ | ✅ | ✅ PASS | 37.7s | 9 |  |
| real-debug-api | trigger-positive | ✅ | ✅ | ✅ PASS | 30.6s | 5 |  |
| neg-npm-install | trigger-negative | ✅ | ✅ | ✅ PASS | 12.1s | 3 |  |
| neg-docker-build | trigger-negative | ✅ | ✅ | ✅ PASS | 29.4s | 8 |  |
| neg-refactor | trigger-negative | ✅ | ✅ | ✅ PASS | 71.3s | 12 |  |
| neg-aws-s3 | trigger-negative | ✅ | ✅ | ✅ PASS | 5.8s | 2 |  |
| neg-existing-key | trigger-negative | ✅ | ✅ | ✅ PASS | 12.8s | 3 |  |
| neg-db-query | trigger-negative | ✅ | ✅ | ✅ PASS | 12.0s | 3 |  |
| neg-ci-pipeline | trigger-negative | ✅ | ✅ | ✅ PASS | 18.3s | 6 |  |
| ambig-no-api-key-generic | trigger-positive | ✅ | ✅ | ✅ PASS | 26.8s | 5 |  |
| ambig-scrape-vs-browse | trigger-negative | ✅ | ✅ | ✅ PASS | 8.2s | 2 |  |
| ambig-help-flag | trigger-positive | ✅ | ✅ | ✅ PASS | 24.4s | 5 |  |
| ambig-cost-question | trigger-negative | ✅ | ✅ | ✅ PASS | 16.1s | 4 |  |

## Failures

### neg-internal-api

**Prompt:** Call our internal staging API at https://staging.internal.corp/api/status to check deployment health.

**Reasons:**
-  tempo-walletinvoked but should not have been (1 calls)

** Tempo Walletcalls:** 1 | **Curl calls:** 0

**Agent response:**
> Would you like me to try any of these alternatives?

