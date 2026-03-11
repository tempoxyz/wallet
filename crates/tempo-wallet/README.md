# tempo-wallet

Wallet identity and custody extension for the Tempo CLI. Manages authentication, key lifecycle, and funding.

## Commands

| Command | Description |
|---------|-------------|
| `tempo wallet login` | Connect via browser passkey authentication |
| `tempo wallet logout` | Disconnect your wallet |
| `tempo wallet whoami` | Show wallet address, balances, keys, and readiness |
| `tempo wallet keys` | List keys with balance and spending limit details |
| `tempo wallet fund` | Fund your wallet (testnet faucet or mainnet bridge) |
| `tempo wallet sessions list` | List payment sessions |
| `tempo wallet services` | Browse the MPP service directory |
| `tempo wallet mpp-sign` | Sign an MPP payment challenge |

## Usage

```bash
# Install
curl -fsSL https://cli.tempo.xyz/install | bash

# Connect your wallet
tempo wallet login

# Check status
tempo wallet whoami
```

## License

Dual-licensed under [Apache 2.0](../../LICENSE-APACHE) and [MIT](../../LICENSE-MIT).
