# tempo-wallet

Wallet identity and custody extension for the Tempo CLI. Manages wallet creation, authentication, key management, and funding.

## Commands

| Command | Description |
|---------|-------------|
| `tempo wallet login` | Connect via browser passkey authentication |
| `tempo wallet logout` | Disconnect your wallet |
| `tempo wallet whoami` | Show wallet address, balances, keys, and readiness |
| `tempo wallet keys` | List keys with balance and spending limit details |
| `tempo wallet wallets create` | Create a new local wallet |
| `tempo wallet wallets list` | List configured wallets |
| `tempo wallet completions <SHELL>` | Generate shell completions |

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
