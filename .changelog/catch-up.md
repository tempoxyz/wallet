---
presto: minor
---

### Features
- Support `@filename` and `@-` (stdin) syntax for `-d`/`--data` flag
- Improved CLI ergonomics (shorter flags, better defaults)
- Gas estimation for Tempo transactions with account creation cost handling
- Key spending limit checks before payment and swap
- Local access key generation (no longer received from browser)
- Analytics with PostHog for funnel dashboards
- Examples directory for common usage patterns
- Eval framework for testing

### Fixes
- SSE streaming no longer hangs
- Atomic file writes for config and wallet files
- Gas estimation uses fixed buffer instead of percentage
- Gas limit bumped to 500k for Account Abstraction transactions
- Show AI integrations message only on new install

### Refactors
- Renamed to presto
- Streamlined CLI — removed config/version commands, merged keys into whoami
- Removed keystore/private key support, passkey auth only
- Cleaned up payment error messages
