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
use tempo_common::payment::session::submit_tempo_tx;

sol! {
    #[sol(rpc)]
    interface ITIP20 {
        function transfer(address to, uint256 amount) external returns (bool);
        function balanceOf(address account) external view returns (uint256);
        function decimals() external view returns (uint8);
        function symbol() external view returns (string);
    }
}

// ---------------------------------------------------------------------------
// Token resolution
// ---------------------------------------------------------------------------

/// A resolved token: address, display symbol, and decimals.
struct ResolvedToken {
    address: Address,
    symbol: String,
    decimals: u8,
}

/// Resolve a `0x`-prefixed token address, querying symbol and decimals on-chain.
///
/// Accepts both `0x…` and `tempox0x…` formats.
async fn resolve_token(input: &str, provider: &impl Provider) -> Result<ResolvedToken> {
    let address = tempo_common::security::parse_address_input(input, "token address")?;

    let contract = ITIP20::new(address, provider);

    let decimals = contract
        .decimals()
        .call()
        .await
        .context("Failed to query token decimals")?;

    let symbol = contract
        .symbol()
        .call()
        .await
        .map(|s| s.to_string())
        .unwrap_or_else(|_| format!("{:#x}", address));

    Ok(ResolvedToken {
        address,
        symbol,
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

    // Validate recipient address early (no network needed)
    let to_address = tempo_common::security::parse_address_input(&to, "recipient address")?;

    let rpc_url = ctx.config.rpc_url(ctx.network);
    let provider = alloy::providers::ProviderBuilder::new().connect_http(rpc_url.clone());

    // Resolve token
    let token = resolve_token(&token_input, &provider).await?;

    // Resolve amount
    let (amount_atomic, amount_human) = resolve_amount(&amount, &token, from, &provider).await?;

    // Resolve fee token (default: same token as transfer)
    let fee_token_address = if let Some(ref ft) = fee_token_input {
        let ft_resolved = resolve_token(ft, &provider).await?;
        ft_resolved.address
    } else {
        token.address
    };

    // Dry run
    if dry_run {
        let response = TransferResponse {
            status: "dry_run",
            tx_hash: None,
            amount: amount_human,
            symbol: token.symbol,
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
                response.amount,
                response.symbol,
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
            .mark_provisioned(ctx.network, ctx.keys.wallet_address());
    }

    let tx_url = ctx.network.tx_url(&tx_hash);

    let response = TransferResponse {
        status: "success",
        tx_hash: Some(tx_hash),
        amount: amount_human,
        symbol: token.symbol,
        token: format!("{:#x}", token.address),
        to: format!("{:#x}", to_address),
        from: format!("{:#x}", from),
        fee: None,
        blockhash: None,
    };

    output::emit_by_format(ctx.output_format, &response, || {
        eprintln!();
        eprintln!("  Submitted");
        eprintln!("    TX: {}", response.tx_hash.as_deref().unwrap_or(""));
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
