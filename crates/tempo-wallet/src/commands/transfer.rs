//! Transfer tokens to an address.

use alloy::primitives::utils::{format_units, parse_units, ParseUnits};
use alloy::primitives::{Address, Bytes, TxKind, U256};
use alloy::providers::Provider;
use alloy::sol;
use alloy::sol_types::SolCall;
use anyhow::{bail, Context, Result};
use serde::Serialize;
use tempo_primitives::transaction::Call;

use tempo_common::cli::context::Context as CliContext;
use tempo_common::cli::output;
use tempo_common::network::NetworkId;
use tempo_common::payment::session::submit_tempo_tx;

sol! {
    #[sol(rpc)]
    interface ITIP20 {
        function transfer(address to, uint256 amount) external returns (bool);
        function balanceOf(address account) external view returns (uint256);
        function decimals() external view returns (uint8);
    }
}

// ---------------------------------------------------------------------------
// Token resolution
// ---------------------------------------------------------------------------

/// Resolve a token symbol or address to a token address and decimals.
///
/// Built-in symbols: `usdc`, `usdc.e`, `pathusd`.
/// Raw `0x`-prefixed addresses are also accepted (decimals queried on-chain).
struct ResolvedToken {
    address: Address,
    symbol: String,
    decimals: u8,
}

fn resolve_token_builtin(input: &str, network: NetworkId) -> Option<ResolvedToken> {
    let token = network.token();
    let needle = input.to_lowercase();
    // Match against known symbols for the network
    match needle.as_str() {
        s if s == token.symbol.to_lowercase() || s == "usdc.e" || s == "usdc" || s == "pathusd" => {
            let address: Address = token.address.parse().ok()?;
            Some(ResolvedToken {
                address,
                symbol: token.symbol.to_string(),
                decimals: token.decimals,
            })
        }
        _ => None,
    }
}

async fn resolve_token(
    input: &str,
    network: NetworkId,
    provider: &impl Provider,
) -> Result<ResolvedToken> {
    // Try built-in symbol first
    if let Some(resolved) = resolve_token_builtin(input, network) {
        return Ok(resolved);
    }

    // Try as raw address
    tempo_common::security::validate_hex_input(input, "token address")?;
    let address: Address = input
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid token address: {input}"))?;

    // Query decimals on-chain
    let contract = ITIP20::new(address, provider);
    let decimals = contract
        .decimals()
        .call()
        .await
        .context("Failed to query token decimals")?;

    Ok(ResolvedToken {
        address,
        symbol: format!("{:#x}", address),
        decimals,
    })
}

// ---------------------------------------------------------------------------
// Amount parsing
// ---------------------------------------------------------------------------

/// Parse a human amount string into atomic units.
///
/// Supports decimal amounts like "1.00", "50", and the special value "all"
/// which transfers the entire balance.
async fn resolve_amount(
    input: &str,
    token: &ResolvedToken,
    from: Address,
    provider: &impl Provider,
) -> Result<(U256, String)> {
    if input.eq_ignore_ascii_case("all") {
        let balance = query_balance(provider, token.address, from).await?;
        if balance.is_zero() {
            bail!("Balance is zero — nothing to transfer.");
        }
        let human = format_units(balance, token.decimals).expect("decimals <= 77");
        return Ok((balance, human));
    }

    let parsed = parse_units(input, token.decimals)
        .map_err(|_| anyhow::anyhow!("Invalid amount: '{input}'"))?;
    let amount = match parsed {
        ParseUnits::U256(v) => v,
        ParseUnits::I256(v) => {
            if v.is_negative() {
                bail!("Amount must be positive.");
            }
            v.into_raw()
        }
    };

    if amount.is_zero() {
        bail!("Amount must be greater than zero.");
    }

    Ok((amount, input.to_string()))
}

async fn query_balance(provider: &impl Provider, token: Address, account: Address) -> Result<U256> {
    let contract = ITIP20::new(token, provider);
    let balance = contract
        .balanceOf(account)
        .call()
        .await
        .context("Failed to query token balance")?;
    Ok(balance)
}

// ---------------------------------------------------------------------------
// JSON response
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct TransferResponse {
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    tx_hash: Option<String>,
    amount: String,
    symbol: String,
    token: String,
    to: String,
    from: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    fee: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    blockhash: Option<String>,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub(crate) async fn run(
    ctx: &CliContext,
    amount: String,
    token_input: String,
    to: String,
    fee_token_input: Option<String>,
    dry_run: bool,
) -> Result<()> {
    // Ensure wallet is connected
    ctx.keys.ensure_key_for_network(ctx.network)?;

    let wallet = ctx.keys.signer(ctx.network)?;
    let from = wallet.from;

    let rpc_url = ctx.config.rpc_url(ctx.network);
    let provider = alloy::providers::ProviderBuilder::new().connect_http(rpc_url.clone());

    // Resolve token
    let token = resolve_token(&token_input, ctx.network, &provider).await?;

    // Resolve recipient — strip optional `tempox` prefix
    let to_raw = to.strip_prefix("tempox").unwrap_or(&to);
    tempo_common::security::validate_hex_input(to_raw, "recipient address")?;
    let to_address: Address = to_raw
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid recipient address: {to}"))?;

    // Resolve amount
    let (amount_atomic, amount_human) = resolve_amount(&amount, &token, from, &provider).await?;

    // Resolve fee token (default: same token as transfer)
    let fee_token_address = if let Some(ref ft) = fee_token_input {
        let ft_resolved = resolve_token(ft, ctx.network, &provider).await?;
        ft_resolved.address
    } else {
        token.address
    };

    // Dry run
    if dry_run {
        let response = TransferResponse {
            status: "dry_run",
            tx_hash: None,
            amount: amount_human.clone(),
            symbol: token.symbol.clone(),
            token: format!("{:#x}", token.address),
            to: format!("{:#x}", to_address),
            from: format!("{:#x}", from),
            fee: None,
            blockhash: None,
        };

        return output::emit_by_format(ctx.output_format, &response, || {
            eprintln!("[DRY RUN]");
            eprintln!(
                "  Sending {} {} → {}",
                amount_human,
                token.symbol,
                format_address(to_address)
            );
            eprintln!("  From: {}", format_address(from));
            eprintln!("  Fee token: {:#x}", fee_token_address);
            Ok(())
        });
    }

    // Build transfer call
    let transfer_data = Bytes::from(
        ITIP20::transferCall {
            to: to_address,
            amount: amount_atomic,
        }
        .abi_encode(),
    );

    let calls = vec![Call {
        to: TxKind::Call(token.address),
        value: U256::ZERO,
        input: transfer_data,
    }];

    // Print pre-confirmation
    if !ctx.output_format.is_structured() {
        eprintln!(
            "  Sending {} {} → {}",
            amount_human,
            token.symbol,
            format_address(to_address)
        );
    }

    let chain_id = ctx.network.chain_id();
    let tempo_provider =
        alloy::providers::RootProvider::<mpp::client::TempoNetwork>::new_http(rpc_url);

    let tx_hash = submit_tempo_tx(
        &tempo_provider,
        &wallet,
        chain_id,
        fee_token_address,
        from,
        calls,
    )
    .await?;

    // Mark provisioned if this was the first tx
    if !ctx.keys.is_provisioned(ctx.network) {
        ctx.keys
            .mark_provisioned(ctx.network, &format!("{:#x}", from));
    }

    let tx_url = ctx.network.tx_url(&tx_hash);

    let response = TransferResponse {
        status: "success",
        tx_hash: Some(tx_hash.clone()),
        amount: amount_human.clone(),
        symbol: token.symbol.clone(),
        token: format!("{:#x}", token.address),
        to: format!("{:#x}", to_address),
        from: format!("{:#x}", from),
        fee: None,
        blockhash: None,
    };

    output::emit_by_format(ctx.output_format, &response, || {
        eprintln!();
        eprintln!("  Submitted");
        eprintln!("    TX: {}", tx_hash);
        eprintln!("    {}", tx_url);
        Ok(())
    })
}

fn format_address(addr: Address) -> String {
    let s = format!("{:#x}", addr);
    if s.len() > 12 {
        format!("{}…{}", &s[..6], &s[s.len() - 4..])
    } else {
        s
    }
}
