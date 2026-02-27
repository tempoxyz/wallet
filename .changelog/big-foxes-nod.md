---
presto: patch
---

Fixed install script to handle environments where `BASH_SOURCE[0]` is unset by guarding its usage, preventing errors when the script is run via `curl | bash`.
