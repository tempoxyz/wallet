# tempo-mpp

HTTP client extension for the Tempo CLI with built-in [MPP](https://mpp.dev) payment support. Makes HTTP requests and automatically handles `402 Payment Required` responses.

## Commands

| Command | Description |
|---------|-------------|
| `tempo mpp <URL>` | Make an HTTP request with automatic payment |
| `tempo mpp services` | Browse the MPP service directory |
| `tempo mpp services info <ID>` | Show detailed info for a service |
| `tempo mpp sessions list` | List payment sessions |
| `tempo mpp sessions close` | Close sessions or channels |
| `tempo mpp sessions sync --origin <URL>` | Re-sync session state from chain |
| `tempo mpp sessions sync` | Remove stale local sessions |

## Usage

```bash
# Make a paid API request
tempo mpp https://openrouter.mpp.tempo.xyz/v1/chat/completions \
  -X POST --json '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"Hello!"}]}'

# Preview cost without paying
tempo mpp --dry-run https://openrouter.mpp.tempo.xyz/v1/chat/completions \
  -X POST --json '{"model":"openai/gpt-4o-mini","messages":[{"role":"user","content":"Hello!"}]}'

# Browse services
tempo mpp services
```

## License

Dual-licensed under [Apache 2.0](../../LICENSE-APACHE) and [MIT](../../LICENSE-MIT).
