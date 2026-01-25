<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/logo-dark.png">
    <source media="(prefers-color-scheme: light)" srcset="assets/logo-light.png">
    <img src="assets/logo-light.png" alt="PURL">
  </picture>
</p>

# p(ay)URL

A curl-like CLI tool for making HTTP requests with automatic support for payments.

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

Use as `purl <URL> [OPTIONS]` or `purl <COMMAND> [OPTIONS]`

| Example | Command |
|---------|---------|
| Initialize configuration (first time setup) | `purl init` or `purl i` |
| Make a payment request | `purl https://api.example.com/premium-data` |
| Preview payment without executing | `purl -D https://api.example.com/data` |
| Require confirmation before payment | `purl -y https://api.example.com/data` |
| Set maximum payment amount (in atomic units) | `purl -M 10000 https://api.example.com/data` |
| Filter to specific networks | `purl -n base-sepolia https://api.example.com/data` |
| Verbose output with headers | `purl -vi https://api.example.com/data` |
| Multi-level verbosity | `purl -vvv https://api.example.com/data` |
| Quiet mode (suppress output) | `purl -q https://api.example.com/data` or `purl -s https://api.example.com/data` |
| Control color output | `purl --color never https://api.example.com/data` |
| Save output to file | `purl -o output.json https://api.example.com/data` |
| JSON output format | `purl --json-output https://api.example.com/data` |
| Custom headers | `purl -H "Authorization: Bearer token" https://api.example.com/data` |
| Use specific account by name | `purl -a my-wallet https://api.example.com/data` |
| Use specific sender address | `purl --from 0x1234... https://api.example.com/data` |
| Override RPC URL | `purl -r https://my-rpc.com https://api.example.com/data` |
| View configuration | `purl config` or `purl c` |
| View configuration with private keys | `purl config --unsafe-show-private-keys` |
| Disable password caching | `purl --no-cache config --unsafe-show-private-keys` |
| List all payment methods (keystores) | `purl method list` or `purl m list` |
| Create a new payment method | `purl method new my-wallet --generate` |
| Import an existing private key | `purl method import my-wallet` |
| Check wallet balance | `purl balance` or `purl b` |
| Check balance on specific network | `purl balance -n base` |
| Inspect payment requirements | `purl inspect https://api.example.com/data` |
| List supported networks | `purl networks` or `purl n` |
| Generate shell completions | `purl completions bash` or `purl com bash` |

## Installation

**Method 1: Quick install script**

```bash
curl -fsSL https://raw.githubusercontent.com/tempoxyz/purl/main/install.sh | bash
```

**Method 2: Install from source**

```bash
git clone https://github.com/tempoxyz/purl.git
cd purl
cargo install --path .
```

This method requires having Rust installed. See [rustup](https://rustup.rs/) for instructions.

Make sure that `~/.cargo/bin` is on your PATH. One way to do this is by adding the line `export PATH="$HOME/.cargo/bin:$PATH"` to your `~/.bashrc` or `~/.profile`.

## Configuration

Purl uses a configuration file and encrypted keystores for secure wallet management.

### Data Locations

Purl uses platform-native directories:

**macOS:**

- **Configuration**: `~/Library/Application Support/purl/config.toml`
- **Keystores**: `~/Library/Application Support/purl/keystores/`

**Linux:**

- **Configuration**: `~/.config/purl/config.toml`
- **Keystores**: `~/.local/share/purl/keystores/`

**Windows:**

- **Configuration**: `%APPDATA%\purl\config.toml`
- **Keystores**: `%APPDATA%\purl\keystores\`

All keystores use Ethereum keystore v3 format for encrypted storage.

### Initial Setup

Run `purl init` for interactive setup:

```bash
purl init              # Create configuration and generate/import wallets
purl init --force      # Force overwrite existing configuration
```

The setup process will:

1. Ask if you want to configure EVM payment methods
1. Offer to generate a new private key or import an existing one
1. Encrypt your key with a password and save as a keystore

### Configuration File Format

The configuration file (see paths above) references encrypted keystores:

```toml
[evm]
keystore = "/Users/username/.purl/keystores/my-wallet.json"
```

**Note**: EVM keys are stored in encrypted keystores for security.

### Custom Networks and RPC Overrides

Purl includes built-in support for common networks (Ethereum, Base, etc.) with default RPC endpoints. You can customize these or add new networks in your configuration file.

**Override RPC URLs for built-in networks:**

```toml
[evm]
keystore = "/Users/username/.purl/keystores/my-wallet.json"

# Override default RPC URLs for built-in networks
[rpc]
base = "https://my-private-base-rpc.com"
ethereum = "https://my-infura-endpoint.io/v3/key"
```

**Add custom networks:**

```toml
# Add custom networks (e.g., private chains or testnets)
[[networks]]
id = "my-private-chain"
chain_type = "evm"
chain_id = 12345
mainnet = false
display_name = "My Private Chain"
rpc_url = "https://rpc.myprivatechain.com"

[[networks]]
id = "another-testnet"
chain_type = "evm"
chain_id = 99999
mainnet = false
display_name = "Another Testnet"
rpc_url = "https://rpc.anothertestnet.com"
```

**Add custom tokens:**

```toml
# Add custom token addresses for balance checks
[[tokens]]
network = "base"
address = "0x..."
symbol = "MYTOKEN"
name = "My Custom Token"
decimals = 18

[[tokens]]
network = "my-private-chain"
address = "0x..."
symbol = "PTOKEN"
name = "Private Token"
decimals = 6
```

**Built-in networks:**

- EVM: `ethereum`, `ethereum-sepolia`, `base`, `base-sepolia`, `tempo-moderato`, `avalanche`, `avalanche-fuji`, `polygon`, `arbitrum`, `optimism`

Custom networks and RPC overrides are loaded at runtime and merged with built-in defaults.

### Viewing Configuration

View your current configuration using the `config` command:

```bash
# View configuration in text format (default)
purl config

# View configuration as JSON
purl config --output-format json

# View configuration as YAML
purl config --output-format yaml

# View configuration with private keys (⚠️ use with caution)
purl config --unsafe-show-private-keys

# Get a specific configuration value
purl config get evm.keystore

# Validate configuration file
purl config validate
```

Example output:

```
Config file: /Users/username/.config/purl/purl.toml

[evm]
keystore = "/Users/username/.purl/keystores/my-wallet.json"
address = "0xe676e0f661bfe316793a8ad576fe7be02b93bd96"
```

## Payment Method Management

Purl provides commands to manage multiple encrypted wallets (payment methods) without editing configuration files directly.

### List Payment Methods

View all available keystores:

```bash
purl method list
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
purl method new my-wallet --generate
```

This will:

1. Generate a new random private key
1. Display the private key for manual backup
1. Prompt for a password to encrypt the keystore
1. Save the encrypted keystore to the platform keystores directory (see [Data Locations](#data-locations))
1. Show instructions for updating your config file

You can also create a keystore without generating a new key:

```bash
purl method new my-wallet
# You'll be prompted to enter an existing private key
```

### Import Existing Private Key

Import an existing private key into a new encrypted keystore:

```bash
purl method import my-wallet
# You'll be prompted to enter your private key and a password
```

Or provide the private key directly (not recommended for security reasons):

```bash
purl method import my-wallet --private-key 0x1234...
```

### Show Keystore Details

View details of a specific keystore without revealing the private key:

```bash
purl method show my-wallet
```

### Verify Keystore Integrity

Verify that a keystore can be decrypted with your password:

```bash
purl method verify my-wallet
```

### Using a Payment Method

After creating a keystore, update your configuration file to use it:

```bash
# Edit ~/.config/purl/purl.toml
[evm]
keystore = "/Users/username/.purl/keystores/my-wallet.json"
```

Or use `purl init --force` to reconfigure interactively.

### Password Caching

To improve user experience, purl automatically caches keystore passwords in memory for 5 minutes after successful decryption. This means you won't need to re-enter your password for repeated operations within this timeframe.

**How it works:**

- Passwords are cached in-memory only (never written to disk)
- Cache entries automatically expire after 5 minutes
- Failed decryption attempts automatically clear the cached password
- Each keystore has its own cached password (identified by canonical file path)
- Cache is cleared when the process exits

**Managing password cache:**

```bash
# Disable password caching for a specific command
purl --no-cache config --unsafe-show-private-keys
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
1. **Never commit keys to git** - The `.gitignore` should exclude `~/.purl/` and `~/.config/purl/`
1. **Consider password caching security** - On shared systems, disable caching with `--no-cache`

## Shell Completions

Purl supports shell completions for Bash, Zsh, Fish, and PowerShell to make command-line usage faster and more convenient.

### Installing Shell Completions

Generate completions for your shell and save them to the appropriate location:

**Bash:**

```bash
purl completions bash > /usr/local/etc/bash_completion.d/purl
# Or use the alias:
purl com bash > /usr/local/etc/bash_completion.d/purl
```

**Zsh:**

```bash
purl completions zsh > ~/.zfunc/_purl
# Then add to ~/.zshrc if not already present:
fpath=(~/.zfunc $fpath)
autoload -Uz compinit && compinit
```

**Fish:**

```bash
purl completions fish > ~/.config/fish/completions/purl.fish
```

**PowerShell:**

```powershell
purl completions power-shell | Out-File -FilePath $PROFILE -Append
```

After installing, restart your shell or source the configuration file. You'll then be able to use Tab to autocomplete purl commands, flags, and values.

## Command Aliases

Purl provides short aliases for common commands to speed up your workflow:

| Full Command | Alias | Description |
|--------------|-------|-------------|
| `purl init` | `purl i` | Initialize configuration |
| `purl config` | `purl c` | Manage configuration |
| `purl version` | `purl v` | Show version information |
| `purl method` | `purl m` | Manage payment methods |
| `purl completions` | `purl com` | Generate shell completions |
| `purl balance` | `purl b` | Check wallet balance |
| `purl networks` | `purl n` | Manage and inspect networks |

**Examples:**

```bash
purl i --force           # Same as: purl init --force
purl c --output-format json  # Same as: purl config --output-format json
purl m list              # Same as: purl method list
purl com bash            # Same as: purl completions bash
```

## Display Options

Purl offers flexible output control to suit different use cases and preferences.

### Verbosity Levels

Control how much information purl outputs:

```bash
purl <URL>           # Normal output
purl -v <URL>        # Verbose: show detailed request/response info
purl -vv <URL>       # Debug: show even more details (reserved for future use)
purl -vvv <URL>      # Trace: show all possible details (reserved for future use)
```

### Quiet Mode

Suppress all non-essential output:

```bash
purl -q <URL>        # Quiet mode
purl -s <URL>        # Short alias for quiet (curl-compatible)
purl --silent <URL>  # Long alias for quiet
```

### Color Output

Control color in terminal output:

```bash
purl --color auto <URL>    # Auto-detect TTY (default)
purl --color always <URL>  # Always use colors
purl --color never <URL>   # Never use colors
```

Purl respects the `NO_COLOR` environment variable ([no-color.org](https://no-color.org/)):

```bash
NO_COLOR=1 purl <URL>  # Disables colors regardless of --color setting
```

### Output Formatting

Format response output in different ways:

```bash
purl --output-format text <URL>  # Plain text (default)
purl --output-format json <URL>  # Pretty-printed JSON
purl --output-format yaml <URL>  # YAML format

# Combined with display options:
purl -q --color never --output-format json <URL>
```

## Protocols

Purl supports multiple payment protocols for HTTP-based payments.

| Protocol | Description | Supported Networks |
|----------|-------------|-------------------|
| [Web Payment Auth](https://datatracker.ietf.org/doc/draft-ietf-httpauth-payment/) | IETF standard for HTTP authentication-based payments | `ethereum`, `ethereum-sepolia`, `base`, `base-sepolia`, `tempo-moderato`, `avalanche`, `avalanche-fuji`, `polygon`, `arbitrum`, `optimism` |

## Environment Variables

```bash
export PURL_MAX_AMOUNT=10000
export PURL_NETWORK=base-sepolia
export PURL_CONFIRM=true

purl https://api.example.com/data
```

## Documentation

**purl help**

Run `purl --help` to see all available options:

```
A curl-like tool for HTTP-based payment requests

Usage: purl [OPTIONS] [URL]
       purl <COMMAND>

Commands:
  init         Initialize purl configuration
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
  -M, --max-amount <AMOUNT>  Maximum amount willing to pay (in atomic units) [env: PURL_MAX_AMOUNT=]
  -y, --confirm              Require confirmation before paying [env: PURL_CONFIRM=]
  -n, --network <NETWORKS>   Filter to specific networks (comma-separated, e.g. "base,base-sepolia") [env: PURL_NETWORK=]
  -D, --dry-run              Dry run mode - show what would be paid without executing

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
      --keystore <PATH>       Path to encrypted keystore file [env: PURL_KEYSTORE=]
  -a, --account <NAME>        Use keystore by name (without .json extension) [env: PURL_ACCOUNT=]
      --from <ADDRESS>        Specify sender address (uses configured keystore for this address) [env: PURL_FROM=]
      --password <PASSWORD>   Password for keystore decryption [env: PURL_PASSWORD=]
      --password-file <PATH>  Path to file containing keystore password [env: PURL_PASSWORD_FILE=]
      --no-cache              Disable password caching for keystores
      --private-key <KEY>     Raw private key (hex, with or without 0x prefix) [env: PURL_PRIVATE_KEY]

RPC Options:
  -r, --rpc <URL>  Override RPC URL for the request [env: PURL_RPC_URL=]
```

**purl init**

Initialize or reconfigure your purl setup:

```bash
purl init              # Interactive setup
purl init --force      # Force overwrite existing config
```

**purl config**

View your current configuration:

```bash
purl config                                          # Text format (default)
purl config --output-format json                     # JSON format
purl config --output-format yaml                     # YAML format
purl config --unsafe-show-private-keys               # Show private keys (password prompt appears first)
purl config --unsafe-show-private-keys --no-cache-password  # Show keys without caching password
```

## Development Setup

```bash
# Clone repository
git clone https://github.com/tempoxyz/purl.git
cd purl

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
