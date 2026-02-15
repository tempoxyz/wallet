# presto

A wget-like CLI tool for making HTTP requests with automatic support for payments.

**Supported Protocols:**

- [Web Payment Auth](https://datatracker.ietf.org/doc/draft-ietf-httpauth-payment/) - IETF standard for HTTP authentication-based payments

**Contents**

- [Example Usage](#example-usage)
- [Installation](#installation)
- [Configuration](#configuration)
  - [Custom Networks and RPC Overrides](#custom-networks-and-rpc-overrides)
- [Shell Completions](#shell-completions)
- [Command Aliases](#command-aliases)
- [Display Options](#display-options)
- [Protocols](#protocols)
- [Environment Variables](#environment-variables)
- [Documentation](#documentation)
- [Development Setup](#development-setup)

## Example Usage

Use as ` tempo-walletquery <URL> [OPTIONS]` (alias ` tempo-walletq <URL>`) or ` tempo-wallet<COMMAND> [OPTIONS]`

| Example | Command |
|---------|---------|
| Log in (first time setup) | ` tempo-walletlogin` or ` tempo-walletl` |
| Make a payment request | ` tempo-walletquery https://api.example.com/premium-data` |
| Preview payment without executing | ` tempo-walletquery -D https://api.example.com/data` |
| Require confirmation before payment | ` tempo-walletquery -y https://api.example.com/data` |
| Set maximum payment amount (in atomic units) | ` tempo-walletquery -M 10000 https://api.example.com/data` |
| Filter to specific networks | ` tempo-walletquery -n tempo-moderato https://api.example.com/data` |
| Verbose output with headers | ` tempo-walletquery -vi https://api.example.com/data` |
| Multi-level verbosity | ` tempo-walletquery -vvv https://api.example.com/data` |
| Quiet mode (suppress output) | ` tempo-walletquery -q https://api.example.com/data` or ` tempo-walletquery -s https://api.example.com/data` |
| Control color output | ` tempo-walletquery --color never https://api.example.com/data` |
| Save output to file | ` tempo-walletquery -o output.json https://api.example.com/data` |
| JSON output format | ` tempo-walletquery --json-output https://api.example.com/data` |
| Custom headers | ` tempo-walletquery -H "Authorization: Bearer token" https://api.example.com/data` |
| Override RPC URL | ` tempo-walletquery -r https://my-rpc.com https://api.example.com/data` |
| Disable automatic token swaps | ` tempo-walletquery --no-swap https://api.example.com/data` |
| Show wallet status | ` tempo-walletwhoami` |
| Check wallet balance | ` tempo-walletbalance` or ` tempo-walletb` |
| Check balance on specific network | ` tempo-walletbalance -n tempo` |
| Inspect payment requirements | ` tempo-walletinspect https://api.example.com/data` |
| List supported networks | ` tempo-walletnetworks` or ` tempo-walletn` |
| Generate shell completions | ` tempo-walletcompletions bash` or ` tempo-walletcom bash` |

## Installation

**Method 1: Quick install script**

```bash
curl -fsSL https://raw.githubusercontent.com/tempoxyz/presto/main/install.sh | bash
```

**Method 2: Install from source**

```bash
git clone https://github.com/tempoxyz/presto.git
cd presto
cargo install --path .
```

This method requires having Rust installed. See [rustup](https://rustup.rs/) for instructions.

Make sure that `~/.cargo/bin` is on your PATH. One way to do this is by adding the line `export PATH="$HOME/.cargo/bin:$PATH"` to your `~/.bashrc` or `~/.profile`.

## Configuration

 tempo-walletuses a configuration file for wallet management.

### Data Locations

 tempo-walletuses platform-native directories:

**macOS:**

- **Configuration**: `~/Library/Application Support/presto/config.toml`
- **Wallet credentials**: `~/Library/Application Support/presto/wallet.toml`

**Linux:**

- **Configuration**: `~/.config/presto/config.toml`
- **Wallet credentials**: `~/.config/presto/wallet.toml`

**Windows:**

- **Configuration**: `%APPDATA%\presto\config.toml`
- **Wallet credentials**: `%APPDATA%\presto\wallet.toml`

### Initial Setup

Run ` tempo-walletlogin` to connect your wallet:

```bash
 tempo-walletlogin             # Opens browser to connect your Tempo wallet
```

This opens your browser to authenticate with your Tempo wallet using passkeys.

### Custom Networks and RPC Overrides

 tempo-walletincludes built-in support for Tempo networks with default RPC endpoints. You can customize these or add new networks in your configuration file.

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

## Shell Completions

 tempo-walletsupports shell completions for Bash, Zsh, Fish, and PowerShell to make command-line usage faster and more convenient.

### Installing Shell Completions

Generate completions for your shell and save them to the appropriate location:

**Bash:**

```bash
 tempo-walletcompletions bash > /usr/local/etc/bash_completion.d/presto
# Or use the alias:
 tempo-walletcom bash > /usr/local/etc/bash_completion.d/presto
```

**Zsh:**

```bash
 tempo-walletcompletions zsh > ~/.zfunc/_presto
# Then add to ~/.zshrc if not already present:
fpath=(~/.zfunc $fpath)
autoload -Uz compinit && compinit
```

**Fish:**

```bash
 tempo-walletcompletions fish > ~/.config/fish/completions/presto.fish
```

**PowerShell:**

```powershell
 tempo-walletcompletions power-shell | Out-File -FilePath $PROFILE -Append
```

After installing, restart your shell or source the configuration file. You'll then be able to use Tab to autocomplete  tempo-walletcommands, flags, and values.

## Command Aliases

 tempo-walletprovides short aliases for common commands to speed up your workflow:

| Full Command | Alias | Description |
|--------------|-------|-------------|
| ` tempo-walletquery` | ` tempo-walletq` | Make an HTTP request with optional payment |
| ` tempo-walletlogin` | ` tempo-walletl` | Log in to Tempo wallet |
| ` tempo-walletlogout` | | Log out and disconnect wallet |
| ` tempo-walletcompletions` | ` tempo-walletcom` | Generate shell completions |
| ` tempo-walletbalance` | ` tempo-walletb` | Check wallet balance |
| ` tempo-walletnetworks` | ` tempo-walletn` | Manage and inspect networks |
| ` tempo-walletwallet` | ` tempo-walletw` | Tempo wallet management |
| ` tempo-walletkeys` | ` tempo-walletk` | Manage access keys |
| ` tempo-walletservices` | ` tempo-walletsvc` | List available payment services |

**Examples:**

```bash
 tempo-walletq https://example.com # Same as:  tempo-walletquery https://example.com
 tempo-walletl                     # Same as:  tempo-walletlogin
 tempo-walletcom bash              # Same as:  tempo-walletcompletions bash
```

## Display Options

 tempo-walletoffers flexible output control to suit different use cases and preferences.

### Verbosity Levels

Control how much information  tempo-walletoutputs:

```bash
 tempo-walletquery <URL>           # Normal output
 tempo-walletquery -v <URL>        # Verbose: show detailed request/response info
 tempo-walletquery -vv <URL>       # Debug: show even more details (reserved for future use)
 tempo-walletquery -vvv <URL>      # Trace: show all possible details (reserved for future use)
```

### Quiet Mode

Suppress all non-essential output:

```bash
 tempo-walletquery -q <URL>        # Quiet mode
 tempo-walletquery -s <URL>        # Short alias for quiet
 tempo-walletquery --silent <URL>  # Long alias for quiet
```

### Color Output

Control color in terminal output:

```bash
 tempo-walletquery --color auto <URL>    # Auto-detect TTY (default)
 tempo-walletquery --color always <URL>  # Always use colors
 tempo-walletquery --color never <URL>   # Never use colors
```

 tempo-walletrespects the `NO_COLOR` environment variable ([no-color.org](https://no-color.org/)):

```bash
NO_COLOR=1  tempo-walletquery <URL>  # Disables colors regardless of --color setting
```

### Output Formatting

Format response output in different ways:

```bash
 tempo-walletquery --output-format text <URL>  # Plain text (default)
 tempo-walletquery --output-format json <URL>  # Pretty-printed JSON
 tempo-walletquery --output-format yaml <URL>  # YAML format

# Combined with display options:
 tempo-walletquery -q --color never --output-format json <URL>
```

## Protocols

 tempo-walletsupports the Web Payment Auth protocol for HTTP-based payments.

| Protocol | Description | Supported Networks |
|----------|-------------|-------------------|
| [Web Payment Auth](https://datatracker.ietf.org/doc/draft-ietf-httpauth-payment/) | IETF standard for HTTP authentication-based payments | `tempo`, `tempo-moderato` (plus any custom networks you define) |

## Automatic Token Swapping

When a merchant requests payment in a specific stablecoin that you don't have,  tempo-walletcan automatically swap from a stablecoin you do have. This feature is enabled by default.

**How it works:**

1. When a payment is requested,  tempo-walletchecks your balance of the required token
2. If you have sufficient balance, the payment proceeds directly
3. If you don't have enough of the required token,  tempo-walletqueries your balances of other supported stablecoins (pathUSD)
4. If another token has sufficient balance (including 0.5% slippage),  tempo-walletautomatically swaps via the Tempo StablecoinDEX and completes the payment in a single atomic transaction

**Token selection:**

 tempo-walletchecks tokens in order and uses the first one with sufficient balance. The balance check includes a 0.5% slippage buffer to ensure the swap succeeds.

**Atomic execution:**

The swap and payment are executed atomically in a single Tempo transaction containing three calls:
1. Approve the DEX to spend your tokens
2. Swap to the required token
3. Transfer to the merchant

If any step fails, the entire transaction reverts.

**Disabling automatic swaps:**

To disable automatic swapping and require exact token balance:

```bash
 tempo-walletquery --no-swap https://api.example.com/data
```

Or set the environment variable:

```bash
export PRESTO_NO_SWAP=true
```

When swaps are disabled and you don't have enough of the required token,  tempo-walletwill display an error with your current balance and the required amount.

## Environment Variables

```bash
export PRESTO_MAX_AMOUNT=10000
export PRESTO_NETWORK=tempo-moderato
export PRESTO_CONFIRM=true
export PRESTO_NO_SWAP=true

 tempo-walletquery https://api.example.com/data
```

## Documentation

** tempo-wallethelp**

Run ` tempo-wallet--help` to see all available options:

```
A wget-like tool for HTTP-based payment requests

Usage:  tempo-wallet[OPTIONS] [COMMAND]

Commands:
  query        Make an HTTP request with optional payment
  login        Log in to your Tempo wallet
  logout       Log out and disconnect your wallet
  balance      Check wallet balance (uses global --network/-n filter)
  whoami       Show who you are: wallet, balances, access keys
  keys         Manage access keys for Tempo wallet
  networks     Manage and inspect supported networks
  wallet       Tempo wallet management
  services     List available payment services
  inspect      Inspect payment requirements without executing payment
  completions  Generate shell completions script
  help         Print this message or the help of the given subcommand(s)

Options:
  -C, --config <PATH>  Configuration file path
  -h, --help           Print help
  -V, --version        Print version

Payment Options:
  -n, --network <NETWORKS>  Filter to specific networks (comma-separated, e.g. "tempo, tempo-moderato") [env: PRESTO_NETWORK=]

Display Options:
  -v, --verbosity...  Verbosity level (can be used multiple times: -v, -vv, -vvv)
      --color <MODE>  Control color output [default: auto] [possible values: auto, always, never]
  -q, --quiet         Do not print log messages (aliases: -s, --silent) [aliases: -s, --silent]
      --json-output   Format output as JSON (shorthand for --output-format json) [aliases: --jo]
```

** tempo-walletquery**

Run ` tempo-walletquery --help` to see request-specific options:

```
Make an HTTP request with optional payment

Usage:  tempo-walletquery [OPTIONS] <URL>

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
  -n, --network <NETWORKS>   Filter to specific networks (comma-separated, e.g. "tempo, tempo-moderato") [env: PRESTO_NETWORK=]

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

** tempo-walletlogin**

Log in to your Tempo wallet:

```bash
 tempo-walletlogin             # Opens browser to connect your Tempo wallet
```

### Specification

[SPEC.md](SPEC.md) defines the expected CLI behaviors — error message formats, exit codes, and user-facing messages. When contributing, ensure changes conform to the spec.

## Development Setup

```bash
# Clone repository
git clone https://github.com/tempoxyz/presto.git
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
