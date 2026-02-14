<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/logo-dark.png">
    <source media="(prefers-color-scheme: light)" srcset="assets/logo-light.png">
    <img src="assets/logo-light.png" alt="presto">
  </picture>
</p>

# presto

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

Use as `presto query <URL> [OPTIONS]` (alias `presto q <URL>`) or `presto <COMMAND> [OPTIONS]`

| Example | Command |
|---------|---------|
| Log in (first time setup) | `presto login` or `presto l` |
| Make a payment request | `presto query https://api.example.com/premium-data` |
| Preview payment without executing | `presto query -D https://api.example.com/data` |
| Require confirmation before payment | `presto query -y https://api.example.com/data` |
| Set maximum payment amount (in atomic units) | `presto query -M 10000 https://api.example.com/data` |
| Filter to specific networks | `presto query -n tempo-moderato https://api.example.com/data` |
| Verbose output with headers | `presto query -vi https://api.example.com/data` |
| Multi-level verbosity | `presto query -vvv https://api.example.com/data` |
| Quiet mode (suppress output) | `presto query -q https://api.example.com/data` or `presto query -s https://api.example.com/data` |
| Control color output | `presto query --color never https://api.example.com/data` |
| Save output to file | `presto query -o output.json https://api.example.com/data` |
| JSON output format | `presto query --json-output https://api.example.com/data` |
| Custom headers | `presto query -H "Authorization: Bearer token" https://api.example.com/data` |
| Override RPC URL | `presto query -r https://my-rpc.com https://api.example.com/data` |
| Disable automatic token swaps | `presto query --no-swap https://api.example.com/data` |
| Show wallet status | `presto whoami` |
| View configuration | `presto config` or `presto c` |
| View configuration with private keys | `presto config --unsafe-show-private-keys` |
| Disable password caching | `presto --no-cache config --unsafe-show-private-keys` |
| Check wallet balance | `presto balance` or `presto b` |
| Check balance on specific network | `presto balance -n tempo` |
| Inspect payment requirements | `presto inspect https://api.example.com/data` |
| List supported networks | `presto networks` or `presto n` |
| Generate shell completions | `presto completions bash` or `presto com bash` |

## Installation

**Method 1: Quick install script**

```bash
curl -fsSL https://raw.githubusercontent.com/tempoxyz/pget/main/install.sh | bash
```

**Method 2: Install from source**

```bash
git clone https://github.com/tempoxyz/pget.git
cd presto
cargo install --path .
```

This method requires having Rust installed. See [rustup](https://rustup.rs/) for instructions.

Make sure that `~/.cargo/bin` is on your PATH. One way to do this is by adding the line `export PATH="$HOME/.cargo/bin:$PATH"` to your `~/.bashrc` or `~/.profile`.

## Configuration

presto uses a configuration file and encrypted keystores for secure wallet management.

### Data Locations

presto uses platform-native directories:

**macOS:**

- **Configuration**: `~/Library/Application Support/presto/config.toml`
- **Keystores**: `~/Library/Application Support/presto/keystores/`

**Linux:**

- **Configuration**: `~/.config/presto/config.toml`
- **Keystores**: `~/.local/share/presto/keystores/`

**Windows:**

- **Configuration**: `%APPDATA%\presto\config.toml`
- **Keystores**: `%APPDATA%\presto\keystores\`

All keystores use Ethereum keystore v3 format for encrypted storage.

### Initial Setup

Run `presto login` to connect your wallet:

```bash
presto login             # Opens browser to connect your Tempo wallet
```

This opens your browser to authenticate with your Tempo wallet using passkeys.

### Advanced: Local Keystores

For CI/automation or if you prefer local key management, use the `method` command:

```bash
presto method new my-wallet --generate   # Generate a new local private key
presto method import my-wallet           # Import an existing private key
```

The keystore flow will:

1. Offer to generate a new private key or import an existing one
2. Encrypt your key with a password and save as a keystore

### Configuration File Format

The configuration file (see paths above) references encrypted keystores:

```toml
[evm]
keystore = "/Users/username/.presto/keystores/my-wallet.json"
```

**Note**: EVM keys are stored in encrypted keystores for security.

### Custom Networks and RPC Overrides

presto includes built-in support for Tempo networks with default RPC endpoints. You can customize these or add new networks in your configuration file.

**Built-in networks:**

| Network | ID | Chain ID | Type | Default RPC |
|---------|------|----------|------|-------------|
| Tempo | `tempo` | 4217 | Mainnet | https://rpc.tempo.xyz |
| Tempo Moderato | `tempo-moderato` | 42431 | Testnet | https://rpc.moderato.tempo.xyz |

**Built-in tokens** (available on all Tempo networks):

| Token | Address |
|-------|---------|
| pathUSD | `0x20c0000000000000000000000000000000000000` |

**Override RPC URLs for built-in networks:**

```toml
[evm]
keystore = "/Users/username/.presto/keystores/my-wallet.json"

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
presto config

# View configuration as JSON
presto config --output-format json

# View configuration as YAML
presto config --output-format yaml

# View configuration with private keys (⚠️ use with caution)
presto config --unsafe-show-private-keys

# Get a specific configuration value
presto config get evm.keystore

# Validate configuration file
presto config validate
```

Example output:

```
Config file: /Users/username/.config/presto/presto.toml

[evm]
keystore = "/Users/username/.presto/keystores/my-wallet.json"
address = "0xe676e0f661bfe316793a8ad576fe7be02b93bd96"
```

## Payment Method Management

presto provides commands to manage multiple encrypted wallets (payment methods) without editing configuration files directly.

### List Payment Methods

View all available keystores:

```bash
presto method list
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
presto method new my-wallet --generate
```

This will:

1. Generate a new random private key
1. Display the private key for manual backup
1. Prompt for a password to encrypt the keystore
1. Save the encrypted keystore to the platform keystores directory (see [Data Locations](#data-locations))
1. Show instructions for updating your config file

You can also create a keystore without generating a new key:

```bash
presto method new my-wallet
# You'll be prompted to enter an existing private key
```

### Import Existing Private Key

Import an existing private key into a new encrypted keystore:

```bash
presto method import my-wallet
# You'll be prompted to enter your private key and a password
```

Or provide the private key directly (not recommended for security reasons):

```bash
presto method import my-wallet --private-key 0x1234...
```

### Show Keystore Details

View details of a specific keystore without revealing the private key:

```bash
presto method show my-wallet
```

### Verify Keystore Integrity

Verify that a keystore can be decrypted with your password:

```bash
presto method verify my-wallet
```

### Using a Payment Method

After creating a keystore, update your configuration file to use it:

```bash
# Edit ~/.config/presto/presto.toml
[evm]
keystore = "/Users/username/.presto/keystores/my-wallet.json"
```

Or use `presto login` to reconfigure interactively.

### Password Caching

To improve user experience, presto automatically caches keystore passwords in memory for 5 minutes after successful decryption. This means you won't need to re-enter your password for repeated operations within this timeframe.

**How it works:**

- Passwords are cached in-memory only (never written to disk)
- Cache entries automatically expire after 5 minutes
- Failed decryption attempts automatically clear the cached password
- Each keystore has its own cached password (identified by canonical file path)
- Cache is cleared when the process exits

**Managing password cache:**

```bash
# Disable password caching for a specific command
presto --no-cache config --unsafe-show-private-keys
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
1. **Never commit keys to git** - The `.gitignore` should exclude `~/.presto/` and `~/.config/presto/`
1. **Consider password caching security** - On shared systems, disable caching with `--no-cache`

## Shell Completions

presto supports shell completions for Bash, Zsh, Fish, and PowerShell to make command-line usage faster and more convenient.

### Installing Shell Completions

Generate completions for your shell and save them to the appropriate location:

**Bash:**

```bash
presto completions bash > /usr/local/etc/bash_completion.d/presto
# Or use the alias:
presto com bash > /usr/local/etc/bash_completion.d/presto
```

**Zsh:**

```bash
presto completions zsh > ~/.zfunc/_presto
# Then add to ~/.zshrc if not already present:
fpath=(~/.zfunc $fpath)
autoload -Uz compinit && compinit
```

**Fish:**

```bash
presto completions fish > ~/.config/fish/completions/presto.fish
```

**PowerShell:**

```powershell
presto completions power-shell | Out-File -FilePath $PROFILE -Append
```

After installing, restart your shell or source the configuration file. You'll then be able to use Tab to autocomplete presto commands, flags, and values.

## Command Aliases

presto provides short aliases for common commands to speed up your workflow:

| Full Command | Alias | Description |
|--------------|-------|-------------|
| `presto query` | `presto q` | Make an HTTP request with optional payment |
| `presto login` | `presto l` | Log in to Tempo wallet |
| `presto logout` | | Log out and disconnect wallet |
| `presto config` | `presto c` | Manage configuration |
| `presto version` | `presto v` | Show version information |
| `presto completions` | `presto com` | Generate shell completions |
| `presto balance` | `presto b` | Check wallet balance |
| `presto networks` | `presto n` | Manage and inspect networks |
| `presto wallet` | `presto w` | Tempo wallet management |
| `presto keys` | `presto k` | Manage access keys |
| `presto services` | `presto svc` | List available payment services |

**Examples:**

```bash
presto q https://example.com # Same as: presto query https://example.com
presto l                     # Same as: presto login
presto c --output-format json  # Same as: presto config --output-format json
presto com bash              # Same as: presto completions bash
```

## Display Options

presto offers flexible output control to suit different use cases and preferences.

### Verbosity Levels

Control how much information presto outputs:

```bash
presto query <URL>           # Normal output
presto query -v <URL>        # Verbose: show detailed request/response info
presto query -vv <URL>       # Debug: show even more details (reserved for future use)
presto query -vvv <URL>      # Trace: show all possible details (reserved for future use)
```

### Quiet Mode

Suppress all non-essential output:

```bash
presto query -q <URL>        # Quiet mode
presto query -s <URL>        # Short alias for quiet
presto query --silent <URL>  # Long alias for quiet
```

### Color Output

Control color in terminal output:

```bash
presto query --color auto <URL>    # Auto-detect TTY (default)
presto query --color always <URL>  # Always use colors
presto query --color never <URL>   # Never use colors
```

presto respects the `NO_COLOR` environment variable ([no-color.org](https://no-color.org/)):

```bash
NO_COLOR=1 presto query <URL>  # Disables colors regardless of --color setting
```

### Output Formatting

Format response output in different ways:

```bash
presto query --output-format text <URL>  # Plain text (default)
presto query --output-format json <URL>  # Pretty-printed JSON
presto query --output-format yaml <URL>  # YAML format

# Combined with display options:
presto query -q --color never --output-format json <URL>
```

## Protocols

presto supports the Web Payment Auth protocol for HTTP-based payments.

| Protocol | Description | Supported Networks |
|----------|-------------|-------------------|
| [Web Payment Auth](https://datatracker.ietf.org/doc/draft-ietf-httpauth-payment/) | IETF standard for HTTP authentication-based payments | `tempo`, `tempo-moderato` (plus any custom networks you define) |

## Automatic Token Swapping

When a merchant requests payment in a specific stablecoin that you don't have, presto can automatically swap from a stablecoin you do have. This feature is enabled by default.

**How it works:**

1. When a payment is requested, presto checks your balance of the required token
2. If you have sufficient balance, the payment proceeds directly
3. If you don't have enough of the required token, presto queries your balances of other supported stablecoins (pathUSD)
4. If another token has sufficient balance (including 0.5% slippage), presto automatically swaps via the Tempo StablecoinDEX and completes the payment in a single atomic transaction

**Token selection:**

presto checks tokens in order and uses the first one with sufficient balance. The balance check includes a 0.5% slippage buffer to ensure the swap succeeds.

**Atomic execution:**

The swap and payment are executed atomically in a single Tempo transaction containing three calls:
1. Approve the DEX to spend your tokens
2. Swap to the required token
3. Transfer to the merchant

If any step fails, the entire transaction reverts.

**Disabling automatic swaps:**

To disable automatic swapping and require exact token balance:

```bash
presto query --no-swap https://api.example.com/data
```

Or set the environment variable:

```bash
export PRESTO_NO_SWAP=true
```

When swaps are disabled and you don't have enough of the required token, presto will display an error with your current balance and the required amount.

## Environment Variables

```bash
export PRESTO_MAX_AMOUNT=10000
export PRESTO_NETWORK=tempo-moderato
export PRESTO_CONFIRM=true
export PRESTO_NO_SWAP=true

presto query https://api.example.com/data
```

## Documentation

**presto help**

Run `presto --help` to see all available options:

```
A wget-like tool for HTTP-based payment requests

Usage: presto [OPTIONS] [COMMAND]

Commands:
  query        Make an HTTP request with optional payment
  login        Log in to your Tempo wallet
  logout       Log out and disconnect your wallet
  config       Manage configuration
  version      Show version information
  completions  Generate shell completions script
  balance      Check wallet balance (uses global --network/-n filter)
  networks     Manage and inspect supported networks
  inspect      Inspect payment requirements without executing payment
  wallet       Tempo wallet management
  keys         Manage access keys for Tempo wallet
  services     List available payment services
  whoami       Show who you are: wallet, balances, access keys
  help         Print this message or the help of the given subcommand(s)

Options:
  -C, --config <PATH>  Configuration file path
  -h, --help           Print help
  -V, --version        Print version

Payment Options:
  -n, --network <NETWORKS>  Filter to specific networks (comma-separated, e.g. "base,base-sepolia") [env: PRESTO_NETWORK=]

Display Options:
  -v, --verbosity...  Verbosity level (can be used multiple times: -v, -vv, -vvv)
      --color <MODE>  Control color output [default: auto] [possible values: auto, always, never]
  -q, --quiet         Do not print log messages (aliases: -s, --silent) [aliases: -s, --silent]
      --json-output   Format output as JSON (shorthand for --output-format json) [aliases: --jo]
```

**presto query**

Run `presto query --help` to see request-specific options:

```
Make an HTTP request with optional payment

Usage: presto query [OPTIONS] <URL>

Arguments:
  <URL>  URL to request

Options:
  -C, --config <PATH>  Configuration file path
  -h, --help           Print help

Payment Options:
  -M, --max-amount <AMOUNT>  Maximum amount willing to pay (in atomic units) [env: PRESTO_MAX_AMOUNT=] [aliases: --max]
  -y, --confirm              Require confirmation before paying [env: PRESTO_CONFIRM=]
  -D, --dry-run              Dry run mode - show what would be paid without executing
      --no-swap              Disable automatic token swaps when you don't have the requested currency [env: PRESTO_NO_SWAP=]
  -n, --network <NETWORKS>   Filter to specific networks (comma-separated, e.g. "base,base-sepolia") [env: PRESTO_NETWORK=]

Request Options:
  -k, --insecure  Allow insecure operations (skip TLS verification and origin checks)

Display Options:
  -i, --include                 Include HTTP headers in output
  -I, --head                    Show only HTTP headers
      --output-format <FORMAT>  Output format for response [default: text] [possible values: text, json, yaml]
  -o, --output <FILE>           Write output to file
  -v, --verbosity...            Verbosity level (can be used multiple times: -v, -vv, -vvv)
      --color <MODE>            Control color output [default: auto] [possible values: auto, always, never]
  -q, --quiet                   Do not print log messages (aliases: -s, --silent) [aliases: -s, --silent]
      --json-output             Format output as JSON (shorthand for --output-format json) [aliases: --jo]

HTTP Options:
  -X, --request <METHOD>           Custom request method
  -H, --header <HEADER>            Add custom header
  -A, --user-agent <AGENT>         Set user agent
  -L, --location                   Follow redirects
      --connect-timeout <SECONDS>  Connection timeout in seconds
  -m, --max-time <SECONDS>         Maximum time for the request
  -d, --data <DATA>                POST data
      --json <JSON>                Send JSON data with Content-Type header

RPC Options:
  -r, --rpc <URL>  Override RPC URL for the request [env: PRESTO_RPC_URL=] [aliases: --rpc-url]
```

**presto login**

Log in to your Tempo wallet:

```bash
presto login             # Opens browser to connect your Tempo wallet
```

**presto config**

View your current configuration:

```bash
presto config                                          # Text format (default)
presto config --output-format json                     # JSON format
presto config --output-format yaml                     # YAML format
presto config --unsafe-show-private-keys               # Show private keys (password prompt appears first)
presto config --unsafe-show-private-keys --no-cache-password  # Show keys without caching password
```

### Specification

[SPEC.md](SPEC.md) defines the expected CLI behaviors — error message formats, exit codes, and user-facing messages. When contributing, ensure changes conform to the spec.

## Development Setup

```bash
# Clone repository
git clone https://github.com/tempoxyz/pget.git
cd presto

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
