# tempo-mpp

MPP session and service management extension for the Tempo CLI. Browse available services and manage payment sessions and channels.

## Commands

| Command | Description |
|---------|-------------|
| `tempo mpp services` | Browse the MPP service directory |
| `tempo mpp services info <ID>` | Show detailed info for a service |
| `tempo mpp sessions list` | List payment sessions |
| `tempo mpp sessions close` | Close sessions or channels |
| `tempo mpp sessions sync --origin <URL>` | Re-sync session state from chain |
| `tempo mpp sessions sync` | Remove stale local sessions |

## Usage

```bash
# Browse services
tempo mpp services

# List sessions
tempo mpp sessions list
```

## License

Dual-licensed under [Apache 2.0](../../LICENSE-APACHE) and [MIT](../../LICENSE-MIT).
