---
presto: minor
---

Updated the install script to use `~/.local/bin` as the default installation directory instead of `/usr/local/bin`, removing the need for sudo. Added automatic PATH configuration for bash and zsh shell rc files, and added cleanup of legacy binaries from the old install location. Also updated passkey auth URLs for the tempo and moderato networks.
