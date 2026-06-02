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
| `tempo wallet cards` | Issue and manage Tempo wallet-backed cards |
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

## Cards

```bash
tempo wallet cards config bridge-api-key sk-test-...
tempo wallet cards config stripe-api-key sk_test_...

tempo wallet cards customers create -f John -l Doe -e john@example.com
tempo wallet cards customers tos-acceptance-link <bridge-customer-id>
tempo wallet cards customers kyc-link <bridge-customer-id> --endorsement cards
tempo wallet cards customers get <bridge-customer-id>

tempo wallet cards create \
  --cardholder <stripe-cardholder-id> \
  --bridge-customer-id <bridge-customer-id>

tempo wallet cards approve --amount max
```

API keys can also come from `BRIDGE_API_KEY`, `TEMPO_BRIDGE_API_KEY`, `STRIPE_SECRET_KEY`, `STRIPE_API_KEY`, or `TEMPO_STRIPE_API_KEY`.

## License

Dual-licensed under [Apache 2.0](../../LICENSE-APACHE) and [MIT](../../LICENSE-MIT).
