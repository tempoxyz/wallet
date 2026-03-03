---
presto: patch
---

Added automatic update checking that fetches the latest version from the release CDN at most once every 6 hours and prints an upgrade notice if a newer version is available. Refactored config loading to happen once at startup and pass it through the call stack, removing redundant `load_config_with_overrides` calls throughout command handlers. Also removed the natural-language prompt forwarding to the `claude` CLI.
