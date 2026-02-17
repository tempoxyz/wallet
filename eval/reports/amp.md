#  Tempo WalletSkill Eval Report

## Summary

| Metric | Value |
|--------|-------|
| Total cases | 56 |
| Passed | 53 |
| Failed | 3 |
| Trigger accuracy | 94% |
| Usage accuracy | 100% |
| Avg duration | 19.4s |
| Avg turns | 2.9 |
| Wall time | 5m10s |

## Results by Category

| Category | Passed | Total | Rate |
|----------|--------|-------|------|
| trigger-negative | 24 | 24 | 100% |
| trigger-positive | 29 | 32 | 90% |

## All Cases

| Case | Category | Trigger | Usage | Result | Duration | Turns | Notes |
|------|----------|---------|-------|--------|----------|-------|-------|
| llm-ask-gpt | trigger-positive | ✅ | ✅ | ✅ PASS | 28.3s | 7 |  |
| llm-ask-claude | trigger-positive | ❌ | ✅ | ❌ FAIL | 6.1s | 2 | expected  tempo-walletinvocation but none found in Bash commands |
| llm-openrouter | trigger-positive | ✅ | ✅ | ✅ PASS | 12.4s | 3 |  |
| llm-generic-no-key | trigger-positive | ✅ | ✅ | ✅ PASS | 21.2s | 5 |  |
| llm-with-spending-limit | trigger-positive | ✅ | ✅ | ✅ PASS | 16.8s | 3 |  |
| llm-dry-run | trigger-positive | ✅ | ✅ | ✅ PASS | 11.7s | 3 |  |
| api-generic-http | trigger-positive | ✅ | ✅ | ✅ PASS | 10.3s | 3 |  |
| wallet-check-balance | trigger-positive | ✅ | ✅ | ✅ PASS | 7.9s | 3 |  |
| wallet-whoami | trigger-positive | ✅ | ✅ | ✅ PASS | 10.0s | 3 |  |
| wallet-login | trigger-positive | ✅ | ✅ | ✅ PASS | 11.0s | 3 |  |
| session-list | trigger-positive | ✅ | ✅ | ✅ PASS | 9.8s | 3 |  |
| api-post-json | trigger-positive | ✅ | ✅ | ✅ PASS | 10.2s | 3 |  |
| api-verbose | trigger-positive | ✅ | ✅ | ✅ PASS | 10.3s | 3 |  |
| api-save-output | trigger-positive | ✅ | ✅ | ✅ PASS | 8.8s | 3 |  |
| api-services-directory | trigger-positive | ✅ | ✅ | ✅ PASS | 20.3s | 3 |  |
| neg-read-file | trigger-negative | ✅ | ✅ | ✅ PASS | 7.4s | 2 |  |
| neg-git-status | trigger-negative | ✅ | ✅ | ✅ PASS | 4.9s | 2 |  |
| neg-write-code | trigger-negative | ✅ | ✅ | ✅ PASS | 7.3s | 2 |  |
| neg-local-build | trigger-negative | ✅ | ✅ | ✅ PASS | 7.9s | 2 |  |
| neg-grep-code | trigger-negative | ✅ | ✅ | ✅ PASS | 6.6s | 2 |  |
| neg-explain-code | trigger-negative | ✅ | ✅ | ✅ PASS | 10.9s | 3 |  |
| neg-math | trigger-negative | ✅ | ✅ | ✅ PASS | 2.5s | 1 |  |
| neg-edit-file | trigger-negative | ✅ | ✅ | ✅ PASS | 9.4s | 3 |  |
| neg-web-search | trigger-negative | ✅ | ✅ | ✅ PASS | 27.1s | 4 |  |
| neg-local-server | trigger-negative | ✅ | ✅ | ✅ PASS | 6.3s | 2 |  |
| ambig-implicit-llm | trigger-positive | ✅ | ✅ | ✅ PASS | 17.3s | 3 |  |
| ambig-summarize-url | trigger-negative | ✅ | ✅ | ✅ PASS | 17.1s | 3 |  |
| ambig-public-api | trigger-negative | ✅ | ✅ | ✅ PASS | 12.1s | 2 |  |
| ambig-presto-the-word | trigger-negative | ✅ | ✅ | ✅ PASS | 16.4s | 3 |  |
| ambig-curl-explicit | trigger-positive | ✅ | ✅ | ✅ PASS | 12.6s | 3 |  |
| usage-custom-headers | trigger-positive | ✅ | ✅ | ✅ PASS | 8.4s | 3 |  |
| usage-quiet-mode | trigger-positive | ✅ | ✅ | ✅ PASS | 9.1s | 3 |  |
| usage-network-filter | trigger-positive | ✅ | ✅ | ✅ PASS | 7.7s | 3 |  |
| usage-combined-flags | trigger-positive | ✅ | ✅ | ✅ PASS | 11.6s | 3 |  |
| usage-include-headers | trigger-positive | ✅ | ✅ | ✅ PASS | 10.4s | 3 |  |
| usage-session-close | trigger-positive | ✅ | ✅ | ✅ PASS | 8.6s | 3 |  |
| neg-github-api | trigger-negative | ✅ | ✅ | ✅ PASS | 9.7s | 3 |  |
| neg-internal-api | trigger-negative | ✅ | ✅ | ✅ PASS | 9.1s | 2 |  |
| real-web-search | trigger-positive | ✅ | ✅ | ✅ PASS | 33.7s | 7 |  |
| real-generate-tests | trigger-positive | ✅ | ✅ | ✅ PASS | 36.0s | 6 |  |
| real-compare-models | trigger-positive | ✅ | ✅ | ✅ PASS | 43.8s | 5 |  |
| real-crawl-site | trigger-positive | ✅ | ✅ | ✅ PASS | 20.4s | 5 |  |
| real-tts-audio | trigger-positive | ❌ | ❌ | ❌ FAIL | 180.0s | 0 | timeout (180s) |
| real-json-output | trigger-positive | ❌ | ❌ | ❌ FAIL | 180.0s | 0 | timeout (180s) |
| real-debug-api | trigger-positive | ✅ | ✅ | ✅ PASS | 24.5s | 4 |  |
| neg-npm-install | trigger-negative | ✅ | ✅ | ✅ PASS | 14.1s | 4 |  |
| neg-docker-build | trigger-negative | ✅ | ✅ | ✅ PASS | 12.2s | 2 |  |
| neg-refactor | trigger-negative | ✅ | ✅ | ✅ PASS | 14.4s | 3 |  |
| neg-aws-s3 | trigger-negative | ✅ | ✅ | ✅ PASS | 4.2s | 1 |  |
| neg-existing-key | trigger-negative | ✅ | ✅ | ✅ PASS | 8.1s | 2 |  |
| neg-db-query | trigger-negative | ✅ | ✅ | ✅ PASS | 8.9s | 2 |  |
| neg-ci-pipeline | trigger-negative | ✅ | ✅ | ✅ PASS | 15.1s | 3 |  |
| ambig-no-api-key-generic | trigger-positive | ✅ | ✅ | ✅ PASS | 20.8s | 3 |  |
| ambig-scrape-vs-browse | trigger-negative | ✅ | ✅ | ✅ PASS | 13.7s | 2 |  |
| ambig-help-flag | trigger-positive | ✅ | ✅ | ✅ PASS | 6.4s | 2 |  |
| ambig-cost-question | trigger-negative | ✅ | ✅ | ✅ PASS | 12.9s | 3 |  |

## Failures

### llm-ask-claude

**Prompt:** Use the Anthropic API to ask Claude what a monad is. Short answer only.

**Reasons:**
- expected  tempo-walletinvocation but none found in Bash commands

** Tempo Walletcalls:** 0 | **Curl calls:** 0

**Agent response:**
> No `ANTHROPIC_API_KEY` is set in the environment. Do you have an API key I should use, or would you prefer I use the **presto** skill (which can call APIs without a key, paying automatically)?

### real-tts-audio

**Prompt:** Convert the text 'Hello, welcome to the demo' to speech audio using ElevenLabs and save the output to demo.mp3. I don't have an ElevenLabs key.

**Reasons:**
- timeout (180s)

** Tempo Walletcalls:** 0 | **Curl calls:** 0

### real-json-output

**Prompt:** Ask GPT-4o-mini to return a JSON object with 3 creative project name suggestions. I need the raw JSON output with no extra logging, piped through jq.

**Reasons:**
- timeout (180s)

** Tempo Walletcalls:** 0 | **Curl calls:** 0

