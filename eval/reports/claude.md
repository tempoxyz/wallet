# Presto Skill Eval Report

## Summary

| Metric | Value |
|--------|-------|
| Total cases | 56 |
| Passed | 55 |
| Failed | 1 |
| Trigger accuracy | 98% |
| Usage accuracy | 100% |
| Avg duration | 19.6s |
| Avg turns | 3.9 |
| Wall time | 5m05s |

## Results by Category

| Category | Passed | Total | Rate |
|----------|--------|-------|------|
| trigger-negative | 24 | 24 | 100% |
| trigger-positive | 31 | 32 | 96% |

## All Cases

| Case | Category | Trigger | Usage | Result | Duration | Turns | Notes |
|------|----------|---------|-------|--------|----------|-------|-------|
| llm-ask-gpt | trigger-positive | ✅ | ✅ | ✅ PASS | 29.5s | 5 |  |
| llm-ask-claude | trigger-positive | ✅ | ✅ | ✅ PASS | 28.7s | 6 |  |
| llm-openrouter | trigger-positive | ✅ | ✅ | ✅ PASS | 72.3s | 17 |  |
| llm-generic-no-key | trigger-positive | ✅ | ✅ | ✅ PASS | 26.5s | 6 |  |
| llm-with-spending-limit | trigger-positive | ✅ | ✅ | ✅ PASS | 21.0s | 5 |  |
| llm-dry-run | trigger-positive | ✅ | ✅ | ✅ PASS | 18.8s | 5 |  |
| api-generic-http | trigger-positive | ✅ | ✅ | ✅ PASS | 10.2s | 4 |  |
| wallet-check-balance | trigger-positive | ✅ | ✅ | ✅ PASS | 6.8s | 2 |  |
| wallet-whoami | trigger-positive | ✅ | ✅ | ✅ PASS | 10.2s | 4 |  |
| wallet-login | trigger-positive | ✅ | ✅ | ✅ PASS | 12.6s | 5 |  |
| session-list | trigger-positive | ✅ | ✅ | ✅ PASS | 9.9s | 2 |  |
| api-post-json | trigger-positive | ✅ | ✅ | ✅ PASS | 10.5s | 4 |  |
| api-verbose | trigger-positive | ✅ | ✅ | ✅ PASS | 12.3s | 4 |  |
| api-save-output | trigger-positive | ✅ | ✅ | ✅ PASS | 15.3s | 4 |  |
| api-services-directory | trigger-positive | ✅ | ✅ | ✅ PASS | 21.8s | 4 |  |
| neg-read-file | trigger-negative | ✅ | ✅ | ✅ PASS | 5.5s | 2 |  |
| neg-git-status | trigger-negative | ✅ | ✅ | ✅ PASS | 7.3s | 2 |  |
| neg-write-code | trigger-negative | ✅ | ✅ | ✅ PASS | 2.9s | 1 |  |
| neg-local-build | trigger-negative | ✅ | ✅ | ✅ PASS | 10.8s | 4 |  |
| neg-grep-code | trigger-negative | ✅ | ✅ | ✅ PASS | 3.8s | 2 |  |
| neg-explain-code | trigger-negative | ✅ | ✅ | ✅ PASS | 5.2s | 2 |  |
| neg-math | trigger-negative | ✅ | ✅ | ✅ PASS | 1.7s | 1 |  |
| neg-edit-file | trigger-negative | ✅ | ✅ | ✅ PASS | 5.5s | 2 |  |
| neg-web-search | trigger-negative | ✅ | ✅ | ✅ PASS | 15.8s | 2 |  |
| neg-local-server | trigger-negative | ✅ | ✅ | ✅ PASS | 6.4s | 2 |  |
| ambig-implicit-llm | trigger-positive | ✅ | ✅ | ✅ PASS | 16.3s | 4 |  |
| ambig-summarize-url | trigger-negative | ✅ | ✅ | ✅ PASS | 16.3s | 2 |  |
| ambig-public-api | trigger-negative | ✅ | ✅ | ✅ PASS | 10.7s | 2 |  |
| ambig-presto-the-word | trigger-negative | ✅ | ✅ | ✅ PASS | 14.4s | 6 |  |
| ambig-curl-explicit | trigger-positive | ✅ | ✅ | ✅ PASS | 21.9s | 6 |  |
| usage-custom-headers | trigger-positive | ✅ | ✅ | ✅ PASS | 12.4s | 4 |  |
| usage-quiet-mode | trigger-positive | ✅ | ✅ | ✅ PASS | 12.4s | 4 |  |
| usage-network-filter | trigger-positive | ✅ | ✅ | ✅ PASS | 14.3s | 4 |  |
| usage-combined-flags | trigger-positive | ✅ | ✅ | ✅ PASS | 15.0s | 4 |  |
| usage-include-headers | trigger-positive | ✅ | ✅ | ✅ PASS | 12.8s | 4 |  |
| usage-session-close | trigger-positive | ✅ | ✅ | ✅ PASS | 9.3s | 4 |  |
| neg-github-api | trigger-negative | ✅ | ✅ | ✅ PASS | 11.0s | 2 |  |
| neg-internal-api | trigger-negative | ✅ | ✅ | ✅ PASS | 4.0s | 1 |  |
| real-web-search | trigger-positive | ✅ | ✅ | ✅ PASS | 45.5s | 6 |  |
| real-generate-tests | trigger-positive | ✅ | ✅ | ✅ PASS | 46.9s | 6 |  |
| real-compare-models | trigger-positive | ✅ | ✅ | ✅ PASS | 37.4s | 8 |  |
| real-crawl-site | trigger-positive | ✅ | ✅ | ✅ PASS | 26.2s | 6 |  |
| real-tts-audio | trigger-positive | ❌ | ❌ | ❌ FAIL | 180.0s | 0 | timeout (180s) |
| real-json-output | trigger-positive | ✅ | ✅ | ✅ PASS | 43.7s | 9 |  |
| real-debug-api | trigger-positive | ✅ | ✅ | ✅ PASS | 33.5s | 6 |  |
| neg-npm-install | trigger-negative | ✅ | ✅ | ✅ PASS | 15.2s | 4 |  |
| neg-docker-build | trigger-negative | ✅ | ✅ | ✅ PASS | 7.7s | 4 |  |
| neg-refactor | trigger-negative | ✅ | ✅ | ✅ PASS | 10.2s | 4 |  |
| neg-aws-s3 | trigger-negative | ✅ | ✅ | ✅ PASS | 5.1s | 2 |  |
| neg-existing-key | trigger-negative | ✅ | ✅ | ✅ PASS | 12.1s | 3 |  |
| neg-db-query | trigger-negative | ✅ | ✅ | ✅ PASS | 8.9s | 2 |  |
| neg-ci-pipeline | trigger-negative | ✅ | ✅ | ✅ PASS | 14.9s | 5 |  |
| ambig-no-api-key-generic | trigger-positive | ✅ | ✅ | ✅ PASS | 28.4s | 6 |  |
| ambig-scrape-vs-browse | trigger-negative | ✅ | ✅ | ✅ PASS | 7.9s | 2 |  |
| ambig-help-flag | trigger-positive | ✅ | ✅ | ✅ PASS | 13.4s | 2 |  |
| ambig-cost-question | trigger-negative | ✅ | ✅ | ✅ PASS | 16.8s | 4 |  |

## Failures

### real-tts-audio

**Prompt:** Convert the text 'Hello, welcome to the demo' to speech audio using ElevenLabs and save the output to demo.mp3. I don't have an ElevenLabs key.

**Reasons:**
- timeout (180s)

**Presto calls:** 0 | **Curl calls:** 0

