---
tempo-wallet: patch
---

Improve close command progress output: add status messages when closing local sessions, sessions by URL, and orphaned channels, and refactor `finalize_closed_channels` to use iterator filtering with a count message before finalizing.
