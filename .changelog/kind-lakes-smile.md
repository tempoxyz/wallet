---
presto: patch
---

Added TOON format support as a compact, token-efficient output and input option. Introduced `-t`/`--toon-output` flag for TOON-formatted output (recommended for agents) and `--toon <TOON>` option to send TOON-encoded request bodies decoded to JSON. Updated agent skill documentation to prefer `-t` over `-j` for token efficiency.
