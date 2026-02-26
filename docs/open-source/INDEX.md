# Open Source Readiness Tasks (OSR)

This index lists all individual task docs. Delete a task file when it is completed and merged.

Phase 1: Baseline + CI Hardening
- [OSR-01: Enforce Lints and Safety Gates](tasks/OSR-01-lints-and-safety-gates.md)
- [OSR-02: CI Coverage, Audit, Deny, MSRV](tasks/OSR-02-ci-coverage-audit-deny-msrv.md)
- [OSR-03: Version Stamping and Reproducible Builds](tasks/OSR-03-version-stamping-reproducible-builds.md)

Phase 2: Agent-First CLI and Output
- [OSR-04: Standardize Machine-Readable Outputs](tasks/OSR-04-standardize-machine-readable-outputs.md)
- [OSR-05: Deterministic Errors and Exit Codes](tasks/OSR-05-deterministic-errors-exit-codes.md)
- [OSR-16: Output Rendering Tests](tasks/OSR-16-output-rendering-tests.md)
- [OSR-06: Help UX Without Leaking Hidden Commands](tasks/OSR-06-help-ux-hidden-commands.md)
- [OSR-19: Input Validation and URL Safety](tasks/OSR-19-input-validation-url-safety.md)
- [OSR-07: Retries, Timeouts, Backoff Flags](tasks/OSR-07-retries-timeouts-backoff.md)
- [OSR-08: Streaming Protocol Contract](tasks/OSR-08-streaming-protocol-contract.md)
- [OSR-21: SAST with Semgrep and CodeQL (informational-only)](tasks/OSR-21-sast-semgrep-codeql.md)

Phase 3: Coverage Expansion
- [OSR-09: Config Parsing and Network Resolution Tests](tasks/OSR-09-config-network-tests.md)
- [OSR-10: Wallet Credentials Model and IO Tests](tasks/OSR-10-wallet-credentials-io-tests.md)
- [OSR-11: Key Authorization Decode/Validate/Sign Tests](tasks/OSR-11-key-authorization-tests.md)
- [OSR-12: HTTP Client and 402→Payment→Response Flow](tasks/OSR-12-http-402-flow-tests.md)
- [OSR-13: Payment Protocol Tests](tasks/OSR-13-payment-protocol-tests.md)
- [OSR-14: Session List/Close Commands](tasks/OSR-14-session-list-close-tests.md)
- [OSR-15: Analytics/Telemetry Tests](tasks/OSR-15-analytics-telemetry-tests.md)
- [OSR-29: Offline Mode and Deterministic Mocks](tasks/OSR-29-offline-mode-mocks.md)

Phase 4: Security Depth
- [OSR-17: Redaction and Logging Guardrails](tasks/OSR-17-redaction-logging-guardrails.md)
- [OSR-18: Secrets Handling Improvements](tasks/OSR-18-secrets-handling-zeroize.md)
- [OSR-20: Fuzzing Targets with cargo-fuzz](tasks/OSR-20-fuzzing-targets-cargo-fuzz.md)
- [OSR-21: SAST with Semgrep and CodeQL](tasks/OSR-21-sast-semgrep-codeql.md)
- [OSR-22: Supply Chain Checks](tasks/OSR-22-supply-chain-checks.md)

Phase 5: Docs, Examples, Release
- [OSR-23: SKILL.md Revamp for AI Agents](tasks/OSR-23-skill-md-revamp.md)
- [OSR-24: README and CLI Reference](tasks/OSR-24-readme-cli-reference.md)
- [OSR-25: SECURITY.md, CONTRIBUTING.md, CODE_OF_CONDUCT.md](tasks/OSR-25-security-contrib-coc.md)
- [OSR-26: Examples for Agents](tasks/OSR-26-examples-for-agents.md)
- [OSR-27: Release Checklist and Changelog](tasks/OSR-27-release-checklist-changelog.md)

Phase 6: Post-GA Niceties
- [OSR-28: CLI Introspection Gated by Env](tasks/OSR-28-cli-introspection-gated.md)
- [OSR-30: Telemetry Schema and Sampling](tasks/OSR-30-telemetry-schema-sampling.md)
