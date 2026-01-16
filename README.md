# `purl` - p(ay)URL

A curl-like CLI tool for making HTTP requests with automatic support for payments. Supports x402 based payments on EVM and Solana.

**Contents**
- [Example Usage](#example-usage)
- [Installation](#installation)
- [Configuration](#configuration)
  - [Custom Networks and RPC Overrides](#custom-networks-and-rpc-overrides)
- [Payment Method Management](#payment-method-management)
- [Shell Completions](#shell-completions)
- [Command Aliases](#command-aliases)
- [Display Options](#display-options)
- [Supported Networks](#supported-networks)
- [Environment Variables](#environment-variables)
- [Documentation](#documentation)
- [Development Setup](#development-setup)

## Example Usage

Use as `purl <URL> [OPTIONS]` or `purl <COMMAND> [OPTIONS]`

| Example | Command |
|---------|---------|
| Initialize configuration (first time setup) | `purl init` or `purl i` |
| Make a payment request | `purl https://api.example.com/premium-data` |
| Preview payment without executing | `purl --dry-run https://api.example.com/data` |
| Require confirmation before payment | `purl --confirm https://api.example.com/data` |
| Set maximum payment amount (in atomic units) | `purl --max-amount 10000 https://api.example.com/data` |
| Filter to specific networks | `purl --network base-sepolia https://api.example.com/data` |
| Verbose output with headers | `purl -vi https://api.example.com/data` |
| Multi-level verbosity | `purl -vvv https://api.example.com/data` |
| Quiet mode (suppress output) | `purl -q https://api.example.com/data` or `purl -s https://api.example.com/data` |
| Control color output | `purl --color never https://api.example.com/data` |
| Save output to file | `purl -o output.json https://api.example.com/data` |
| JSON output format | `purl --output-format json https://api.example.com/data` |
| Custom headers | `purl -H "Authorization: Bearer token" https://api.example.com/data` |
| View configuration | `purl config` or `purl c` |
| View configuration with private keys | `purl config --unsafe-show-private-keys` |
| Disable password caching | `purl --no-cache-password config --unsafe-show-private-keys` |
| List all payment methods (keystores) | `purl method list` or `purl m list` |
| Create a new payment method | `purl method new my-wallet --generate` |
| Import an existing private key | `purl method import my-wallet` |
| Generate shell completions | `purl completions bash` or `purl com bash` |

## Installation

**Method 1: Quick install script**

```bash
curl -fsSL https://raw.githubusercontent.com/brendanjryan/purl/main/install.sh | bash
```

**Method 2: Install from source**

```bash
git clone https://github.com/brendanjryan/purl.git
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
- **Cache**: `~/Library/Caches/purl/password_cache/`

**Linux:**
- **Configuration**: `~/.config/purl/config.toml`
- **Keystores**: `~/.local/share/purl/keystores/`
- **Cache**: `~/.cache/purl/password_cache/`

**Windows:**
- **Configuration**: `%APPDATA%\purl\config.toml`
- **Keystores**: `%APPDATA%\purl\keystores\`
- **Cache**: `%LOCALAPPDATA%\purl\password_cache\`

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
1. Optionally configure Solana payment methods

### Configuration File Format

The configuration file (see paths above) references encrypted keystores:

```toml
[evm]
keystore = "/Users/username/.purl/keystores/my-wallet.json"

[solana]
private_key = "your_base58_encoded_keypair"
```

**Note**: EVM keys are stored in encrypted keystores (recommended), while Solana keys are currently stored in the config file. Keystore encryption support for Solana is planned.

### Custom Networks and RPC Overrides

Purl includes built-in support for common networks (Ethereum, Base, Solana, etc.) with default RPC endpoints. You can customize these or add new networks in your configuration file.

**Override RPC URLs for built-in networks:**

```toml
[evm]
keystore = "/Users/username/.purl/keystores/my-wallet.json"

[solana]
keystore = "/Users/username/.purl/keystores/solana-wallet.json"

# Override default RPC URLs for built-in networks
[rpc]
base = "https://my-private-base-rpc.com"
ethereum = "https://my-infura-endpoint.io/v3/key"
solana = "https://my-helius-endpoint.com"
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
- EVM: `ethereum`, `ethereum-sepolia`, `base`, `base-sepolia`, `avalanche`, `avalanche-fuji`, `polygon`, `arbitrum`, `optimism`
- Solana: `solana`, `solana-devnet`

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
```

Example output:
```
Config file: /Users/username/.config/purl/purl.toml

[evm]
keystore = "/Users/username/.purl/keystores/my-wallet.json"
address = "0xe676e0f661bfe316793a8ad576fe7be02b93bd96"

[solana]
private_key = "5JGg...KwRb"
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
1. Save the encrypted keystore to `~/.purl/keystores/my-wallet.json`
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

### Using a Payment Method

After creating a keystore, update your configuration file to use it:

```bash
# Edit ~/.config/purl/purl.toml
[evm]
keystore = "/Users/username/.purl/keystores/my-wallet.json"
```

Or use `purl init --force` to reconfigure interactively.

### Password Caching

To improve user experience, purl automatically caches keystore passwords for 5 minutes after successful decryption. This means you won't need to re-enter your password for repeated operations within this timeframe.

**How it works:**
- Passwords are cached in `~/.purl/.password_cache/` with a timestamp
- Cache entries automatically expire after 5 minutes
- Failed decryption attempts automatically clear the cached password
- Each keystore has its own cached password (identified by canonical file path)

**Managing password cache:**

```bash
# Disable password caching for a specific command
purl --no-cache-password config --unsafe-show-private-keys

# Clear all cached passwords (useful when switching accounts or for security)
rm -rf ~/.purl/.password_cache/
```

**Security considerations:**
- Cache files are stored in your home directory with standard file permissions
- Passwords are stored in plain text in the cache (protected only by filesystem permissions)
- For maximum security, use `--no-cache-password` flag or clear the cache regularly
- Cache directory is automatically created and managed by purl

### Security Best Practices

1. **Always use keystores for EVM wallets** - Encrypted keystores are more secure than plain private keys
1. **Use strong passwords** - Your keystore is only as secure as your password
1. **Back up your keystores** - Keep encrypted copies of `~/.purl/keystores/` in a secure location
1. **Save your private keys** - When generating new keys, save them securely (you'll need them to recover your wallet)
1. **Never commit keys to git** - The `.gitignore` should exclude `~/.purl/` and `~/.config/purl/`
1. **Consider password caching security** - On shared systems, disable caching with `--no-cache-password` or clear the cache after use

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
| `purl config` | `purl c` | Show configuration |
| `purl version` | `purl v` | Show version information |
| `purl method` | `purl m` | Manage payment methods |
| `purl completions` | `purl com` | Generate shell completions |

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

## Supported Networks

| EVM | SVM |
|-----|-----|
| `ethereum`, `ethereum-sepolia`, `base`, `base-sepolia`, `avalanche`, `avalanche-fuji`, `polygon`, `arbitrum`, `optimism` | `solana`, `solana-devnet` |

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
  init     Initialize purl configuration
  config   Show current configuration (with obfuscated keys)
  version  Show version information
  method   Manage payment methods (keystores)
  help     Print this message or the help of the given subcommand(s)

Arguments:
  [URL]  URL to request

Options:
      --max-amount <AMOUNT>        Maximum amount willing to pay (in atomic units) [env: PURL_MAX_AMOUNT=]
      --confirm                    Require confirmation before paying [env: PURL_CONFIRM=]
      --network <NETWORKS>         Filter to specific networks (comma-separated, e.g. "base,base-sepolia") Overrides configured payment methods [env: PURL_NETWORK=]
      --dry-run                    Dry run mode - show what would be paid without executing
      --output-format <FORMAT>     Output format for response [default: text] [possible values: text, json, yaml]
  -v, --verbose                    Verbose mode (equivalent to curl -v)
  -i, --include                    Include HTTP headers in output (equivalent to curl -i)
  -I, --head                       Show only HTTP headers (equivalent to curl -I)
  -X, --request <METHOD>           Custom request method (equivalent to curl -X)
  -H, --header <HEADER>            Add custom header (equivalent to curl -H)
  -A, --user-agent <AGENT>         Set user agent (equivalent to curl -A)
  -L, --location                   Follow redirects (equivalent to curl -L)
      --connect-timeout <SECONDS>  Connection timeout in seconds (equivalent to curl --connect-timeout)
  -m, --max-time <SECONDS>         Maximum time for the request (equivalent to curl -m/--max-time)
  -o, --output <FILE>              Write output to file (equivalent to curl -o)
  -s, --silent                     Silent mode (equivalent to curl -s)
  -d, --data <DATA>                POST data (equivalent to curl -d)
      --json <JSON>                Send JSON data with Content-Type header
      --keystore <PATH>            Path to encrypted keystore file [env: PURL_KEYSTORE=]
      --password <PASSWORD>        Password for keystore decryption [env: PURL_PASSWORD=]
      --private-key <KEY>          Raw private key (hex, for EVM; use keystore for better security) [env: PURL_PRIVATE_KEY=]
      --no-cache-password          Disable password caching for keystores
  -C, --config <PATH>              Configuration file path (default: ~/.purl/config.toml)
  -h, --help                       Print help
  -V, --version                    Print version
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
git clone https://github.com/brendanjryan/purl.git
cd purl

# Build
make build

# Run tests
make test

# Build release binary
make release
```
