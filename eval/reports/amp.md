# Presto Skill Eval Report

## Summary

| Metric | Value |
|--------|-------|
| Total cases | 59 |
| Passed | - |
| Failed | - |
| Trigger accuracy | - |
| Usage accuracy | - |
| Avg duration | - |
| Avg turns | - |
| Wall time | running... |

## Results by Category

_Pending..._

## All Cases

| Case | Category | Trigger | Usage | Result | Duration | Turns | Notes |
|------|----------|---------|-------|--------|----------|-------|-------|
| llm-ask-gpt | trigger-positive | ❌ | ✅ | ❌ FAIL | 8.0s | 1 | expected presto invocation but none found in Bash commands |
| llm-ask-claude | trigger-positive | ❌ | ✅ | ❌ FAIL | 13.1s | 3 | expected presto invocation but none found in Bash commands |
| llm-openrouter | trigger-positive | ✅ | ✅ | ✅ PASS | 17.8s | 3 |  |
| llm-generic-no-key | trigger-positive | ❌ | ✅ | ❌ FAIL | 4.5s | 1 | expected presto invocation but none found in Bash commands |
| llm-with-spending-limit | trigger-positive | ⏳ | ⏳ | ⏳ Running | - | - |  |
| llm-dry-run | trigger-positive | ⏳ | ⏳ | ⏳ Running | - | - |  |
| api-generic-http | trigger-positive | ✅ | ✅ | ✅ PASS | 11.2s | 3 |  |
| wallet-check-balance | trigger-positive | ⏳ | ⏳ | ⏳ Running | - | - |  |
| wallet-whoami | trigger-positive | ⏳ | ⏳ | ⏳ Running | - | - |  |
| wallet-login | trigger-positive | ⏳ | ⏳ | ⏳ Running | - | - |  |
| session-list | trigger-positive | ⏳ | ⏳ | ⏳ Running | - | - |  |
| api-post-json | trigger-positive | ⏳ | ⏳ | ⏳ Running | - | - |  |
| api-verbose | trigger-positive | ⏳ | ⏳ | ⏳ Running | - | - |  |
| api-save-output | trigger-positive | ⏳ | ⏳ | ⏳ Running | - | - |  |
| api-services-directory | trigger-positive | ⏳ | ⏳ | ⏳ Running | - | - |  |
| neg-read-file | trigger-negative | ⏳ | ⏳ | ⏳ Running | - | - |  |
| neg-git-status | trigger-negative | ⏳ | ⏳ | ⏳ Running | - | - |  |
| neg-write-code | trigger-negative | ⏳ | ⏳ | ⏳ Running | - | - |  |
| neg-local-build | trigger-negative | ⏳ | ⏳ | ⏳ Running | - | - |  |
| neg-grep-code | trigger-negative | ⏳ | ⏳ | ⏳ Running | - | - |  |
| neg-explain-code | trigger-negative | ⏳ | ⏳ | ⏳ Running | - | - |  |
| neg-math | trigger-negative | ⏳ | ⏳ | ⏳ Running | - | - |  |
| neg-edit-file | trigger-negative | ⏳ | ⏳ | ⏳ Running | - | - |  |
| neg-web-search | trigger-negative | ⏳ | ⏳ | ⏳ Running | - | - |  |
| neg-local-server | trigger-negative | ⏳ | ⏳ | ⏳ Running | - | - |  |
| ambig-implicit-llm | trigger-positive | ⏳ | ⏳ | ⏳ Running | - | - |  |
| ambig-translate | trigger-positive | ⏳ | ⏳ | ⏳ Running | - | - |  |
| ambig-summarize-url | trigger-negative | ⏳ | ⏳ | ⏳ Running | - | - |  |
| ambig-public-api | trigger-negative | ⏳ | ⏳ | ⏳ Running | - | - |  |
| ambig-presto-the-word | trigger-negative | ⏳ | ⏳ | ⏳ Running | - | - |  |
| ambig-curl-explicit | trigger-positive | ⏳ | ⏳ | ⏳ Running | - | - |  |
| usage-custom-headers | trigger-positive | ⏳ | ⏳ | ⏳ Running | - | - |  |
| usage-quiet-mode | trigger-positive | ⏳ | ⏳ | ⏳ Running | - | - |  |
| usage-network-filter | trigger-positive | ⏳ | ⏳ | ⏳ Running | - | - |  |
| usage-combined-flags | trigger-positive | ⏳ | ⏳ | ⏳ Running | - | - |  |
| usage-include-headers | trigger-positive | ⏳ | ⏳ | ⏳ Running | - | - |  |
| usage-session-close | trigger-positive | ⏳ | ⏳ | ⏳ Running | - | - |  |
| neg-github-api | trigger-negative | ⏳ | ⏳ | ⏳ Running | - | - |  |
| neg-internal-api | trigger-negative | ⏳ | ⏳ | ⏳ Running | - | - |  |
| real-scrape-docs | trigger-positive | ⏳ | ⏳ | ⏳ Running | - | - |  |
| real-web-search | trigger-positive | ⏳ | ⏳ | ⏳ Running | - | - |  |
| real-code-review | trigger-positive | ⏳ | ⏳ | ⏳ Running | - | - |  |
| real-generate-tests | trigger-positive | ⏳ | ⏳ | ⏳ Running | - | - |  |
| real-compare-models | trigger-positive | ⏳ | ⏳ | ⏳ Running | - | - |  |
| real-crawl-site | trigger-positive | ⏳ | ⏳ | ⏳ Running | - | - |  |
| real-tts-audio | trigger-positive | ⏳ | ⏳ | ⏳ Running | - | - |  |
| real-json-output | trigger-positive | ⏳ | ⏳ | ⏳ Running | - | - |  |
| real-debug-api | trigger-positive | ⏳ | ⏳ | ⏳ Running | - | - |  |
| neg-npm-install | trigger-negative | ⏳ | ⏳ | ⏳ Running | - | - |  |
| neg-docker-build | trigger-negative | ⏳ | ⏳ | ⏳ Running | - | - |  |
| neg-refactor | trigger-negative | ⏳ | ⏳ | ⏳ Running | - | - |  |
| neg-aws-s3 | trigger-negative | ⏳ | ⏳ | ⏳ Running | - | - |  |
| neg-existing-key | trigger-negative | ⏳ | ⏳ | ⏳ Running | - | - |  |
| neg-db-query | trigger-negative | ⏳ | ⏳ | ⏳ Running | - | - |  |
| neg-ci-pipeline | trigger-negative | ⏳ | ⏳ | ⏳ Running | - | - |  |
| ambig-no-api-key-generic | trigger-positive | ⏳ | ⏳ | ⏳ Running | - | - |  |
| ambig-scrape-vs-browse | trigger-negative | ⏳ | ⏳ | ⏳ Running | - | - |  |
| ambig-help-flag | trigger-positive | ⏳ | ⏳ | ⏳ Running | - | - |  |
| ambig-cost-question | trigger-negative | ⏳ | ⏳ | ⏳ Running | - | - |  |
