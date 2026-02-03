<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/logo-dark.png">
    <source media="(prefers-color-scheme: light)" srcset="assets/logo-light.png">
    <img src="assets/logo-light.png" alt="pget">
  </picture>
</p>

# pget

A wget-like CLI tool for making HTTP requests with automatic support for payments.

**Supported Protocols:**

- [Web Payment Auth](https://datatracker.ietf.org/doc/draft-ietf-httpauth-payment/) - IETF standard for HTTP authentication-based payments

**Contents**

- [Example Usage](#example-usage)
- [Installation](#installation)
- [Configuration](#configuration)
  - [Custom Networks and RPC Overrides](#custom-networks-and-rpc-overrides)
- [Payment Method Management](#payment-method-management)
- [Shell Completions](#shell-completions)
- [Command Aliases](#command-aliases)
- [Display Options](#display-options)
- [Protocols](#protocols)
- [Environment Variables](#environment-variables)
- [Documentation](#documentation)
- [Development Setup](#development-setup)

## Example Usage

Use as `pget <URL> [OPTIONS]` or `pget <COMMAND> [OPTIONS]`

| Example | Command |
|---------|---------|
| Initialize configuration (first time setup) | `pget init` or `pget i` |
| Make a payment request | `pget https://api.example.com/premium-data` |
| Preview payment without executing | `pget -D https://api.example.com/data` |
| Require confirmation before payment | `pget -y https://api.example.com/data` |
| Set maximum payment amount (in atomic units) | `pget -M 10000 https://api.example.com/data` |
| Filter to specific networks | `pget -n tempo-moderato https://api.example.com/data` |
| Verbose output with headers | `pget -vi https://api.example.com/data` |
| Multi-level verbosity | `pget -vvv https://api.example.com/data` |
| Quiet mode (suppress output) | `pget -q https://api.example.com/data` or `pget -s https://api.example.com/data` |
| Control color output | `pget --color never https://api.example.com/data` |
| Save output to file | `pget -o output.json https://api.example.com/data` |
| JSON output format | `pget --json-output https://api.example.com/data` |
| Custom headers | `pget -H "Authorization: Bearer token" https://api.example.com/data` |
| Use specific account by name | `pget -a my-wallet https://api.example.com/data` |
| Use specific sender address | `pget --from 0x1234... https://api.example.com/data` |
| Override RPC URL | `pget -r https://my-rpc.com https://api.example.com/data` |
| Disable automatic token swaps | `pget --no-swap https://api.example.com/data` |
| View configuration | `pget config` or `pget c` |
| View configuration with private keys | `pget config --unsafe-show-private-keys` |
| Disable password caching | `pget --no-cache config --unsafe-show-private-keys` |
| List all payment methods (keystores) | `pget method list` or `pget m list` |
| Create a new payment method | `pget method new my-wallet --generate` |
| Import an existing private key | `pget method import my-wallet` |
| Check wallet balance | `pget balance` or `pget b` |
| Check balance on specific network | `pget balance -n tempo` |
| Inspect payment requirements | `pget inspect https://api.example.com/data` |
| List supported networks | `pget networks` or `pget n` |
| Generate shell completions | `pget completions bash` or `pget com bash` |

## Installation

**Method 1: Quick install script**

```bash
curl -fsSL https://raw.githubusercontent.com/tempoxyz/pget/main/install.sh | bash
```

**Method 2: Install from source**

```bash
git clone https://github.com/tempoxyz/pget.git
cd pget
cargo install --path .
```

This method requires having Rust installed. See [rustup](https://rustup.rs/) for instructions.

Make sure that `~/.cargo/bin` is on your PATH. One way to do this is by adding the line `export PATH="$HOME/.cargo/bin:$PATH"` to your `~/.bashrc` or `~/.profile`.

## Configuration

pget uses a configuration file and encrypted keystores for secure wallet management.

### Data Locations

pget uses platform-native directories:

**macOS:**

- **Configuration**: `~/Library/Application Support/pget/config.toml`
- **Keystores**: `~/Library/Application Support/pget/keystores/`

**Linux:**

- **Configuration**: `~/.config/pget/config.toml`
- **Keystores**: `~/.local/share/pget/keystores/`

**Windows:**

- **Configuration**: `%APPDATA%\pget\config.toml`
- **Keystores**: `%APPDATA%\pget\keystores\`

All keystores use Ethereum keystore v3 format for encrypted storage.

### Initial Setup

Run `pget init` for interactive setup:

```bash
pget init              # Create configuration and generate/import wallets
pget init --force      # Force overwrite existing configuration
```

The setup process will:

1. Ask if you want to configure EVM payment methods
1. Offer to generate a new private key or import an existing one
1. Encrypt your key with a password and save as a keystore

### Configuration File Format

The configuration file (see paths above) references encrypted keystores:

```toml
[evm]
keystore = "/Users/username/.pget/keystores/my-wallet.json"
```

**Note**: EVM keys are stored in encrypted keystores for security.

### Custom Networks and RPC Overrides

pget includes built-in support for Tempo networks with default RPC endpoints. You can customize these or add new networks in your configuration file.

**Built-in networks:**

| Network | ID | Chain ID | Type | Default RPC |
|---------|------|----------|------|-------------|
| Tempo | `tempo` | 4217 | Mainnet | https://rpc.tempo.xyz |
| Tempo Moderato | `tempo-moderato` | 42431 | Testnet | https://rpc.moderato.tempo.xyz |

**Built-in tokens** (available on all Tempo networks):

| Token | Address |
|-------|---------|
| pathUSD | `0x20c0000000000000000000000000000000000000` |
| AlphaUSD | `0x20c0000000000000000000000000000000000001` |
| BetaUSD | `0x20c0000000000000000000000000000000000002` |
| ThetaUSD | `0x20c0000000000000000000000000000000000003` |

**Override RPC URLs for built-in networks:**

```toml
[evm]
keystore = "/Users/username/.pget/keystores/my-wallet.json"

# Typed RPC overrides for built-in networks (highest priority)
tempo_rpc = "https://my-custom-tempo-rpc.com"
moderato_rpc = "https://my-custom-moderato-rpc.com"

# General RPC overrides (for any network by id)
[rpc]
tempo = "https://alternate-tempo-rpc.com"
"tempo-moderato" = "https://alternate-moderato-rpc.com"
```

**Note:** Typed overrides (`tempo_rpc`, `moderato_rpc`) take precedence over the general `[rpc]` table.

**Add custom networks:**

```toml
# Add custom networks (e.g., private chains or testnets)
[[networks]]
id = "my-private-chain"
chain_id = 12345
mainnet = false
display_name = "My Private Chain"
rpc_url = "https://rpc.myprivatechain.com"
explorer_url = "https://explorer.myprivatechain.com"

[[networks]]
id = "my-tempo-fork"
chain_id = 99999
mainnet = true
display_name = "My Tempo Fork"
rpc_url = "https://rpc.mytempofork.com"
```

Custom networks are checked before built-in networks when resolving, so you can override built-in networks by defining a custom network with the same ID.

### Viewing Configuration

View your current configuration using the `config` command:

```bash
# View configuration in text format (default)
pget config

# View configuration as JSON
pget config --output-format json

# View configuration as YAML
pget config --output-format yaml

# View configuration with private keys (⚠️ use with caution)
pget config --unsafe-show-private-keys

# Get a specific configuration value
pget config get evm.keystore

# Validate configuration file
pget config validate
```

Example output:

```
Config file: /Users/username/.config/pget/pget.toml

[evm]
keystore = "/Users/username/.pget/keystores/my-wallet.json"
address = "0xe676e0f661bfe316793a8ad576fe7be02b93bd96"
```

## Payment Method Management

pget provides commands to manage multiple encrypted wallets (payment methods) without editing configuration files directly.

### List Payment Methods

View all available keystores:

```bash
pget method list
```

Example output:

```
Available keystores:
  my-wallet.json (0xe676e0f661bfe316793a8ad576fe7be02b93bd96)
  trading-wallet.json (0x1234567890123456789012345678901234567890)
  backup-wallet.json (0xabcdefabcdefabcdefabcdefabcdefabcdefabcd)
```

### Create New Payment Method

Generate a new wallet and save it as an encrypted keystore:

```bash
pget method new my-wallet --generate
```

This will:

1. Generate a new random private key
1. Display the private key for manual backup
1. Prompt for a password to encrypt the keystore
1. Save the encrypted keystore to the platform keystores directory (see [Data Locations](#data-locations))
1. Show instructions for updating your config file

You can also create a keystore without generating a new key:

```bash
pget method new my-wallet
# You'll be prompted to enter an existing private key
```

### Import Existing Private Key

Import an existing private key into a new encrypted keystore:

```bash
pget method import my-wallet
# You'll be prompted to enter your private key and a password
```

Or provide the private key directly (not recommended for security reasons):

```bash
pget method import my-wallet --private-key 0x1234...
```

### Show Keystore Details

View details of a specific keystore without revealing the private key:

```bash
pget method show my-wallet
```

### Verify Keystore Integrity

Verify that a keystore can be decrypted with your password:

```bash
pget method verify my-wallet
```

### Using a Payment Method

After creating a keystore, update your configuration file to use it:

```bash
# Edit ~/.config/pget/pget.toml
[evm]
keystore = "/Users/username/.pget/keystores/my-wallet.json"
```

Or use `pget init --force` to reconfigure interactively.

### Password Caching

To improve user experience, pget automatically caches keystore passwords in memory for 5 minutes after successful decryption. This means you won't need to re-enter your password for repeated operations within this timeframe.

**How it works:**

- Passwords are cached in-memory only (never written to disk)
- Cache entries automatically expire after 5 minutes
- Failed decryption attempts automatically clear the cached password
- Each keystore has its own cached password (identified by canonical file path)
- Cache is cleared when the process exits

**Managing password cache:**

```bash
# Disable password caching for a specific command
pget --no-cache config --unsafe-show-private-keys
```

**Security considerations:**

- Cache files are stored in your home directory with standard file permissions
- Passwords are stored in process memory only, never persisted to disk
- For maximum security, use `--no-cache` flag to disable caching entirely

### Security Best Practices

1. **Always use keystores for EVM wallets** - Encrypted keystores are more secure than plain private keys
1. **Use strong passwords** - Your keystore is only as secure as your password
1. **Back up your keystores** - Keep encrypted copies of your keystores directory in a secure location
1. **Save your private keys** - When generating new keys, save them securely (you'll need them to recover your wallet)
1. **Never commit keys to git** - The `.gitignore` should exclude `~/.pget/` and `~/.config/pget/`
1. **Consider password caching security** - On shared systems, disable caching with `--no-cache`

## Shell Completions

pget supports shell completions for Bash, Zsh, Fish, and PowerShell to make command-line usage faster and more convenient.

### Installing Shell Completions

Generate completions for your shell and save them to the appropriate location:

**Bash:**

```bash
pget completions bash > /usr/local/etc/bash_completion.d/pget
# Or use the alias:
pget com bash > /usr/local/etc/bash_completion.d/pget
```

**Zsh:**

```bash
pget completions zsh > ~/.zfunc/_pget
# Then add to ~/.zshrc if not already present:
fpath=(~/.zfunc $fpath)
autoload -Uz compinit && compinit
```

**Fish:**

```bash
pget completions fish > ~/.config/fish/completions/pget.fish
```

**PowerShell:**

```powershell
pget completions power-shell | Out-File -FilePath $PROFILE -Append
```

After installing, restart your shell or source the configuration file. You'll then be able to use Tab to autocomplete pget commands, flags, and values.

## Command Aliases

pget provides short aliases for common commands to speed up your workflow:

| Full Command | Alias | Description |
|--------------|-------|-------------|
| `pget init` | `pget i` | Initialize configuration |
| `pget config` | `pget c` | Manage configuration |
| `pget version` | `pget v` | Show version information |
| `pget method` | `pget m` | Manage payment methods |
| `pget completions` | `pget com` | Generate shell completions |
| `pget balance` | `pget b` | Check wallet balance |
| `pget networks` | `pget n` | Manage and inspect networks |

**Examples:**

```bash
pget i --force           # Same as: pget init --force
pget c --output-format json  # Same as: pget config --output-format json
pget m list              # Same as: pget method list
pget com bash            # Same as: pget completions bash
```

## Display Options

pget offers flexible output control to suit different use cases and preferences.

### Verbosity Levels

Control how much information pget outputs:

```bash
pget <URL>           # Normal output
pget -v <URL>        # Verbose: show detailed request/response info
pget -vv <URL>       # Debug: show even more details (reserved for future use)
pget -vvv <URL>      # Trace: show all possible details (reserved for future use)
```

### Quiet Mode

Suppress all non-essential output:

```bash
pget -q <URL>        # Quiet mode
pget -s <URL>        # Short alias for quiet ()
pget --silent <URL>  # Long alias for quiet
```

### Color Output

Control color in terminal output:

```bash
pget --color auto <URL>    # Auto-detect TTY (default)
pget --color always <URL>  # Always use colors
pget --color never <URL>   # Never use colors
```

pget respects the `NO_COLOR` environment variable ([no-color.org](https://no-color.org/)):

```bash
NO_COLOR=1 pget <URL>  # Disables colors regardless of --color setting
```

### Output Formatting

Format response output in different ways:

```bash
pget --output-format text <URL>  # Plain text (default)
pget --output-format json <URL>  # Pretty-printed JSON
pget --output-format yaml <URL>  # YAML format

# Combined with display options:
pget -q --color never --output-format json <URL>
```

## Protocols

pget supports the Web Payment Auth protocol for HTTP-based payments.

| Protocol | Description | Supported Networks |
|----------|-------------|-------------------|
| [Web Payment Auth](https://datatracker.ietf.org/doc/draft-ietf-httpauth-payment/) | IETF standard for HTTP authentication-based payments | `tempo`, `tempo-moderato` (plus any custom networks you define) |

## Automatic Token Swapping

When a merchant requests payment in a specific stablecoin that you don't have, pget can automatically swap from a stablecoin you do have. This feature is enabled by default.

**How it works:**

1. When a payment is requested, pget checks your balance of the required token
2. If you have sufficient balance, the payment proceeds directly
3. If you don't have enough of the required token, pget queries your balances of other supported stablecoins (pathUSD, AlphaUSD, BetaUSD, ThetaUSD)
4. If another token has sufficient balance (including 0.5% slippage), pget automatically swaps via the Tempo StablecoinDEX and completes the payment in a single atomic transaction

**Token selection:**

pget checks tokens in order and uses the first one with sufficient balance. The balance check includes a 0.5% slippage buffer to ensure the swap succeeds.

**Atomic execution:**

The swap and payment are executed atomically in a single Tempo transaction containing three calls:
1. Approve the DEX to spend your tokens
2. Swap to the required token
3. Transfer to the merchant

If any step fails, the entire transaction reverts.

**Disabling automatic swaps:**

To disable automatic swapping and require exact token balance:

```bash
pget --no-swap https://api.example.com/data
```

Or set the environment variable:

```bash
export PGET_NO_SWAP=true
```

When swaps are disabled and you don't have enough of the required token, pget will display an error with your current balance and the required amount.

## Environment Variables

```bash
export PGET_MAX_AMOUNT=10000
export PGET_NETWORK=tempo-moderato
export PGET_CONFIRM=true
export PGET_NO_SWAP=true

pget https://api.example.com/data
```

## Documentation

**pget help**

Run `pget --help` to see all available options:

```
A wget-like tool for HTTP-based payment requests

Usage: pget [OPTIONS] [URL]
       pget <COMMAND>

Commands:
  init         Initialize pget configuration
  config       Manage configuration
  version      Show version information
  method       Manage payment methods (keystores)
  completions  Generate shell completions script
  balance      Check wallet balance (uses global --network/-n filter)
  networks     Manage and inspect supported networks
  inspect      Inspect payment requirements without executing payment
  help         Print this message or the help of the given subcommand(s)

Arguments:
  [URL]  URL to request

Options:
  -C, --config <PATH>  Configuration file path
  -h, --help           Print help
  -V, --version        Print version

Payment Options:
  -M, --max-amount <AMOUNT>  Maximum amount willing to pay (in atomic units) [env: PGET_MAX_AMOUNT=]
  -y, --confirm              Require confirmation before paying [env: PGET_CONFIRM=]
  -n, --network <NETWORKS>   Filter to specific networks (comma-separated, e.g. "base,base-sepolia") [env: PGET_NETWORK=]
  -D, --dry-run              Dry run mode - show what would be paid without executing
      --no-swap              Disable automatic token swaps [env: PGET_NO_SWAP=]

Display Options:
  -v, --verbosity...            Verbosity level (can be used multiple times: -v, -vv, -vvv)
      --color <MODE>            Control color output [default: auto] [possible values: auto, always, never]
  -q, --quiet                   Do not print log messages (aliases: -s, --silent)
  -i, --include                 Include HTTP headers in output
  -I, --head                    Show only HTTP headers
      --output-format <FORMAT>  Output format for response [default: text] [possible values: text, json, yaml]
      --json-output             Format output as JSON (shorthand for --output-format json)
  -o, --output <FILE>           Write output to file

HTTP Options:
  -X, --request <METHOD>           Custom request method
  -H, --header <HEADER>            Add custom header
  -A, --user-agent <AGENT>         Set user agent
  -L, --location                   Follow redirects
      --connect-timeout <SECONDS>  Connection timeout in seconds
  -m, --max-time <SECONDS>         Maximum time for the request
  -d, --data <DATA>                POST data
      --json <JSON>                Send JSON data with Content-Type header

Wallet Options:
      --keystore <PATH>       Path to encrypted keystore file [env: PGET_KEYSTORE=]
  -a, --account <NAME>        Use keystore by name (without .json extension) [env: PGET_ACCOUNT=]
      --from <ADDRESS>        Specify sender address (uses configured keystore for this address) [env: PGET_FROM=]
      --password <PASSWORD>   Password for keystore decryption [env: PGET_PASSWORD=]
      --password-file <PATH>  Path to file containing keystore password [env: PGET_PASSWORD_FILE=]
      --no-cache              Disable password caching for keystores
      --private-key <KEY>     Raw private key (hex, with or without 0x prefix) [env: PGET_PRIVATE_KEY]

RPC Options:
  -r, --rpc <URL>  Override RPC URL for the request [env: PGET_RPC_URL=]
```

**pget init**

Initialize or reconfigure your pget setup:

```bash
pget init              # Interactive setup
pget init --force      # Force overwrite existing config
```

**pget config**

View your current configuration:

```bash
pget config                                          # Text format (default)
pget config --output-format json                     # JSON format
pget config --output-format yaml                     # YAML format
pget config --unsafe-show-private-keys               # Show private keys (password prompt appears first)
pget config --unsafe-show-private-keys --no-cache-password  # Show keys without caching password
```

## Development Setup

```bash
# Clone repository
git clone https://github.com/tempoxyz/pget.git
cd pget

# Install dependencies (for linting)
npm install

# Build
make build

# Run tests
make test

# Run lints
npm run lint

# Build release binary
make release
```

### Linting

This project uses [Tempo lints](https://github.com/tempoxyz/lints) for code quality checks:

```bash
# Run all Rust lints
npm run lint
```

**Note**: Use `npm` for linting instead of `pnpm`. The `@tempoxyz/lints` package uses build scripts that are blocked by pnpm v10's security features, preventing proper installation of the ast-grep binary.

To disable a lint for a specific line:

```rust
// ast-grep-ignore: no-unwrap-in-lib
let value = something.unwrap();
```
