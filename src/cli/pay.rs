//! Pay command — send an ERC-20 transfer to an address on Tempo.

use std::time::Duration;

use alloy::primitives::{Address, U256};
use alloy::providers::Provider;
use alloy::sol;
use serde::Serialize;

use crate::config::{Config, OutputFormat};
use crate::error::TempoWalletError;
use crate::network::networks::network_or_default;
use crate::network::Network;
use crate::util::format_u256_with_decimals;
use crate::wallet::signer::{load_wallet_signer, WalletSigner};

// ---------------------------------------------------------------------------
// ABI
// ---------------------------------------------------------------------------

sol! {
    interface IERC20 {
        function transfer(address to, uint256 amount) external returns (bool);
    }
}

// ---------------------------------------------------------------------------
// Tx builder constants (match payment/session/tx.rs)
// ---------------------------------------------------------------------------

const MAX_FEE_PER_GAS: u128 = mpp::client::tempo::MAX_FEE_PER_GAS;
const MAX_PRIORITY_FEE_PER_GAS: u128 = mpp::client::tempo::MAX_PRIORITY_FEE_PER_GAS;
const VALID_BEFORE_SECS: u64 = 25;

// ---------------------------------------------------------------------------
// Receipt fetch timeout
// ---------------------------------------------------------------------------

const RECEIPT_FETCH_TIMEOUT_SECS: u64 = 10;

// ---------------------------------------------------------------------------
// JSON response
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
struct PayResponse {
    tx_hash: String,
    to: String,
    amount: String,
    symbol: String,
    network: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    receipt: Option<String>,
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub async fn run_pay(
    config: &Config,
    output_format: OutputFormat,
    network: Option<&str>,
    to: String,
    amount: String,
    currency: Option<String>,
) -> anyhow::Result<()> {
    let network_id = network_or_default(network);
    let net: Network = network_id
        .parse()
        .map_err(|_| anyhow::anyhow!("Unknown network '{network_id}'."))?;

    let wallet = load_wallet_signer(network_id)?;

    let token_config = resolve_token(net, currency.as_deref())?;

    let atomic_amount = parse_amount(&amount, token_config.decimals)?;

    let to_address: Address = to
        .parse()
        .map_err(|e| TempoWalletError::InvalidAddress(format!("{e}")))?;

    let token_address: Address = token_config
        .address
        .parse()
        .map_err(|e| TempoWalletError::InvalidConfig(format!("Invalid token address: {e}")))?;

    let network_info = config.resolve_network(network_id)?;

    if output_format == OutputFormat::Text {
        let formatted = format_u256_with_decimals(atomic_amount, token_config.decimals);
        eprintln!(
            "Sending {} {} to {} on {}...",
            formatted, token_config.symbol, to, network_id
        );
    }

    let tx_hash = submit_transfer(
        &network_info.rpc_url,
        &wallet,
        net.chain_id(),
        token_address,
        to_address,
        atomic_amount,
    )
    .await?;

    // Try to fetch the text receipt from the explorer
    let receipt_text = if let Some(ref explorer) = network_info.explorer {
        let url = format!("{}/receipt/{}.txt", explorer.base_url, tx_hash);
        fetch_text_receipt(&url).await
    } else {
        None
    };

    if output_format.is_structured() {
        let response = PayResponse {
            tx_hash: tx_hash.clone(),
            to: to.clone(),
            amount: amount.clone(),
            symbol: token_config.symbol.to_string(),
            network: network_id.to_string(),
            receipt: receipt_text.clone(),
        };
        println!("{}", output_format.serialize(&response)?);
    } else if let Some(ref text) = receipt_text {
        println!("{text}");
    } else {
        print_local_receipt(&to, &amount, token_config.symbol, &tx_hash, network_id);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Token resolution
// ---------------------------------------------------------------------------

fn resolve_token(
    net: Network,
    currency: Option<&str>,
) -> anyhow::Result<crate::network::TokenConfig> {
    let tokens = net.supported_tokens();

    if let Some(cur) = currency {
        // Match by symbol (case-insensitive) or address
        if let Some(t) = tokens
            .iter()
            .find(|t| t.symbol.eq_ignore_ascii_case(cur) || t.address.eq_ignore_ascii_case(cur))
        {
            return Ok(*t);
        }
        anyhow::bail!(
            "Unknown currency '{cur}' on {net}. Available: {}",
            tokens
                .iter()
                .map(|t| t.symbol)
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    // Default: first token for the network
    tokens
        .first()
        .copied()
        .ok_or_else(|| anyhow::anyhow!("No tokens configured for network {net}"))
}

// ---------------------------------------------------------------------------
// Amount parsing
// ---------------------------------------------------------------------------

fn parse_amount(amount: &str, decimals: u8) -> anyhow::Result<U256> {
    let parts: Vec<&str> = amount.split('.').collect();
    if parts.len() > 2 {
        anyhow::bail!("Invalid amount format: '{amount}'");
    }

    let whole_val: u64 = parts[0]
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid amount: {e}"))?;
    let whole = U256::from(whole_val);

    let frac = if parts.len() == 2 {
        let frac_str = parts[1];
        if frac_str.len() > decimals as usize {
            anyhow::bail!("Too many decimal places (max {decimals}): '{amount}'");
        }
        let padded = format!("{:0<width$}", frac_str, width = decimals as usize);
        let val: u64 = padded
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid fractional amount: {e}"))?;
        U256::from(val)
    } else {
        U256::ZERO
    };

    let divisor = U256::from(10u64).pow(U256::from(decimals));
    Ok(whole * divisor + frac)
}

// ---------------------------------------------------------------------------
// Transaction submission
// ---------------------------------------------------------------------------

async fn submit_transfer(
    rpc_url_str: &str,
    wallet: &WalletSigner,
    chain_id: u64,
    token: Address,
    to: Address,
    amount: U256,
) -> anyhow::Result<String> {
    use alloy::primitives::{Bytes, TxKind};
    use alloy::sol_types::SolCall;
    use mpp::client::tempo::{signing, tx_builder};
    use tempo_primitives::transaction::Call;

    let rpc_url = Network::parse_rpc_url(rpc_url_str)?;
    let provider = alloy::providers::RootProvider::<mpp::client::TempoNetwork>::new_http(rpc_url);

    let call_data = IERC20::transferCall { to, amount }.abi_encode();

    let calls = vec![Call {
        to: TxKind::Call(token),
        value: U256::ZERO,
        input: Bytes::from(call_data),
    }];

    let from = wallet.from;
    let nonce = 0u64;
    let valid_before = Some(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            + VALID_BEFORE_SECS,
    );

    let key_auth = wallet.signing_mode.key_authorization();

    let gas_limit = tx_builder::estimate_gas(
        &provider,
        from,
        chain_id,
        nonce,
        token,
        &calls,
        MAX_FEE_PER_GAS,
        MAX_PRIORITY_FEE_PER_GAS,
        key_auth,
        U256::MAX,
        valid_before,
    )
    .await
    .map_err(|e| TempoWalletError::Signing(e.to_string()))?;

    let tx = tx_builder::build_tempo_tx(tx_builder::TempoTxOptions {
        calls,
        chain_id,
        fee_token: token,
        nonce,
        nonce_key: U256::MAX,
        gas_limit,
        max_fee_per_gas: MAX_FEE_PER_GAS,
        max_priority_fee_per_gas: MAX_PRIORITY_FEE_PER_GAS,
        fee_payer: false,
        valid_before,
        key_authorization: key_auth.cloned(),
    });

    let tx_bytes = signing::sign_and_encode_async(tx, &wallet.signer, &wallet.signing_mode)
        .await
        .map_err(|e| TempoWalletError::Signing(e.to_string()))?;

    let pending = provider
        .send_raw_transaction(&tx_bytes)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to broadcast transaction: {e:#}"))?;

    Ok(format!("{:#x}", pending.tx_hash()))
}

// ---------------------------------------------------------------------------
// Receipt fetching
// ---------------------------------------------------------------------------

async fn fetch_text_receipt(url: &str) -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(RECEIPT_FETCH_TIMEOUT_SECS))
        .build()
        .ok()?;

    let resp = client.get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.text().await.ok()
}

// ---------------------------------------------------------------------------
// Local receipt fallback
// ---------------------------------------------------------------------------

fn print_local_receipt(to: &str, amount: &str, symbol: &str, tx_hash: &str, network: &str) {
    let to_short = shorten_address(to);
    let tx_short = shorten_address(tx_hash);

    let w = 38;
    println!("╔{}╗", "═".repeat(w));
    println!("║{:^w$}║", "TEMPO WALLET RECEIPT");
    println!("╠{}╣", "═".repeat(w));
    println!("║  {:<w2$}║", format!("To:     {to_short}"), w2 = w - 2);
    println!(
        "║  {:<w2$}║",
        format!("Amount: {amount} {symbol}"),
        w2 = w - 2
    );
    println!("║  {:<w2$}║", format!("Tx:     {tx_short}"), w2 = w - 2);
    println!("║  {:<w2$}║", format!("Net:    {network}"), w2 = w - 2);
    println!("╚{}╝", "═".repeat(w));
}

fn shorten_address(addr: &str) -> String {
    if addr.len() > 14 {
        format!("{}...{}", &addr[..6], &addr[addr.len() - 4..])
    } else {
        addr.to_string()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_amount_whole() {
        let result = parse_amount("1", 6).unwrap();
        assert_eq!(result, U256::from(1_000_000u64));
    }

    #[test]
    fn test_parse_amount_fractional() {
        let result = parse_amount("1.50", 6).unwrap();
        assert_eq!(result, U256::from(1_500_000u64));
    }

    #[test]
    fn test_parse_amount_small() {
        let result = parse_amount("0.01", 6).unwrap();
        assert_eq!(result, U256::from(10_000u64));
    }

    #[test]
    fn test_parse_amount_zero() {
        let result = parse_amount("0", 6).unwrap();
        assert_eq!(result, U256::ZERO);
    }

    #[test]
    fn test_parse_amount_too_many_decimals() {
        let result = parse_amount("1.1234567", 6);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_amount_invalid() {
        let result = parse_amount("abc", 6);
        assert!(result.is_err());
    }

    #[test]
    fn test_shorten_address_long() {
        let addr = "0xAbCdEf1234567890AbCdEf1234567890AbCdEf12";
        let short = shorten_address(addr);
        assert_eq!(short, "0xAbCd...Ef12");
    }

    #[test]
    fn test_shorten_address_short() {
        let addr = "0xAbC";
        let short = shorten_address(addr);
        assert_eq!(short, "0xAbC");
    }

    #[test]
    fn test_resolve_token_default_mainnet() {
        let token = resolve_token(Network::Tempo, None).unwrap();
        assert_eq!(token.symbol, "USDC");
    }

    #[test]
    fn test_resolve_token_default_testnet() {
        let token = resolve_token(Network::TempoModerato, None).unwrap();
        assert_eq!(token.symbol, "pathUSD");
    }

    #[test]
    fn test_resolve_token_by_symbol() {
        let token = resolve_token(Network::Tempo, Some("pathusd")).unwrap();
        assert_eq!(token.symbol, "pathUSD");
    }

    #[test]
    fn test_resolve_token_unknown() {
        let result = resolve_token(Network::Tempo, Some("BTC"));
        assert!(result.is_err());
    }
}
