# tempo-wallet

Wallet identity and custody extension for the Tempo CLI. Manages authentication, key lifecycle, and funding.

## Commands

| Command | Description |
|---------|-------------|
| `tempo wallet login` | Connect via browser passkey authentication |
| `tempo wallet login --no-browser` | Remote-host login when the human is on another device |
| `tempo wallet refresh` | Renew your access key without logging out |
| `tempo wallet logout` | Disconnect your wallet |
| `tempo wallet whoami` | Show wallet address, balances, keys, and readiness |
| `tempo wallet keys` | List keys with balance and spending limit details |
| `tempo wallet fund` | Fund your wallet (opens browser) |
| `tempo wallet fund --no-browser` | Remote-host funding when the human is on another device |
| `tempo wallet sessions list` | List payment sessions |
| `tempo wallet sessions close` | Close by origin or channel ID, or batch close/finalize/orphaned |
| `tempo wallet sessions sync` | Reconcile local sessions against on-chain state |
| `tempo wallet services` | Browse the MPP service directory |
| `tempo wallet mpp-sign` | Sign an MPP payment challenge |

## Usage

```bash
# Install
curl -fsSL https://tempo.xyz/install | bash

# Connect your wallet
tempo wallet login

# Remote-host login
tempo wallet login --no-browser

# Check status
tempo wallet whoami

# Fund your wallet
tempo wallet fund

# Remote-host funding
tempo wallet fund --no-browser
```

If you use the remote-host funding path, return to your CLI or agent session once funding is complete so the workflow can continue.

## License

Dual-licensed under [Apache 2.0](../../LICENSE-APACHE) and [MIT](../../LICENSE-MIT).
