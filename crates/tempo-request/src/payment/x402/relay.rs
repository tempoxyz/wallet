//! Relay bridge: quote, execute on-chain steps, and poll for completion.
//!
//! Bridges USDC from Tempo (chain 4217) to the destination chain via
//! the Relay protocol (<https://relay.link>).
//!
//! Bridge transactions are executed as Tempo type-0x76 transactions,
//! supporting both direct EOA and keychain/passkey wallet signers.

use std::time::Duration;

use alloy::{
    primitives::{Address, Bytes, TxKind, U256},
    sol,
};

use tempo_common::{
    config::Config,
    error::{NetworkError, PaymentError, TempoError},
    keys::Signer,
    network::{NetworkId, USDCE_TOKEN},
    payment::session::submit_tempo_tx,
};

sol! {
    #[sol(rpc)]
    interface IERC20 {
        function balanceOf(address account) external view returns (uint256);
    }
}

/// USDC token address on Tempo mainnet (as hex string for Relay API).
const USDC_ON_TEMPO: &str = "0x20c000000000000000000000b9537d11c60e8b50";

/// Relay API base URL.
const RELAY_API: &str = "https://api.relay.link";

/// Maximum number of status polls before giving up.
const MAX_POLL_ATTEMPTS: u32 = 120;

/// Delay between status polls.
const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Tempo mainnet chain ID.
const TEMPO_CHAIN_ID: u64 = 4217;

/// Check the ERC-20 token balance of `account` on a destination chain.
///
/// Returns the balance as `U256`. Uses a public RPC for the destination chain.
pub(super) async fn check_destination_balance(
    dest_chain_id: u64,
    token: Address,
    account: Address,
) -> Result<U256, TempoError> {
    let rpc_url = public_rpc_url(dest_chain_id).ok_or_else(|| PaymentError::ChallengeSchema {
        context: "x402 balance check",
        reason: format!("no public RPC known for chain {dest_chain_id}"),
    })?;

    let url: url::Url = rpc_url.parse().map_err(|_| PaymentError::ChallengeSchema {
        context: "x402 balance check",
        reason: format!("invalid RPC URL for chain {dest_chain_id}"),
    })?;

    let provider = alloy::providers::RootProvider::<alloy::network::Ethereum>::new_http(url);
    let contract = IERC20::new(token, &provider);
    let balance =
        contract
            .balanceOf(account)
            .call()
            .await
            .map_err(|source| NetworkError::RpcSource {
                operation: "check destination token balance",
                source: Box::new(source),
            })?;

    Ok(balance)
}

/// Resolve a public RPC URL for known EVM chains.
fn public_rpc_url(chain_id: u64) -> Option<&'static str> {
    match chain_id {
        1 => Some("https://eth.llamarpc.com"),
        8453 => Some("https://mainnet.base.org"),
        84532 => Some("https://sepolia.base.org"),
        10 => Some("https://mainnet.optimism.io"),
        42161 => Some("https://arb1.arbitrum.io/rpc"),
        137 => Some("https://polygon-rpc.com"),
        _ => None,
    }
}

/// Get a bridge quote from Relay.
///
/// `wallet` is the address holding USDC on Tempo (may be a smart wallet).
/// `recipient` is the address to receive funds on the destination chain
/// (the raw key address, which exists as an EOA on all EVM chains).
pub(super) async fn get_quote(
    http_client: &reqwest::Client,
    wallet: Address,
    recipient: Address,
    dest_chain_id: u64,
    dest_currency: &str,
    amount: &str,
) -> Result<serde_json::Value, TempoError> {
    let body = serde_json::json!({
        "user": format!("{wallet:#x}"),
        "originChainId": TEMPO_CHAIN_ID,
        "destinationChainId": dest_chain_id,
        "originCurrency": USDC_ON_TEMPO,
        "destinationCurrency": dest_currency,
        "amount": amount,
        "tradeType": "EXACT_OUTPUT",
        "recipient": format!("{recipient:#x}"),
    });

    let resp = http_client
        .post(format!("{RELAY_API}/quote/v2"))
        .json(&body)
        .send()
        .await
        .map_err(NetworkError::Reqwest)?;

    let status = resp.status();
    let text = resp.text().await.map_err(NetworkError::Reqwest)?;

    if !status.is_success() {
        return Err(PaymentError::ChallengeSchema {
            context: "Relay quote",
            reason: format!("HTTP {status}: {text}"),
        }
        .into());
    }

    serde_json::from_str(&text).map_err(|source| {
        NetworkError::ResponseParse {
            context: "Relay quote response",
            source,
        }
        .into()
    })
}

/// Execute Relay bridge steps (approve + deposit) on Tempo.
///
/// Iterates through the `steps[]` array from the quote response,
/// converts each step item into a Tempo type-0x76 transaction call,
/// and submits via [`submit_tempo_tx`] which supports both direct EOA
/// and keychain/passkey wallet signers.
pub(super) async fn execute_steps(
    config: &Config,
    signer: &Signer,
    quote: &serde_json::Value,
) -> Result<(), TempoError> {
    let steps = quote
        .get("steps")
        .and_then(|v| v.as_array())
        .ok_or_else(|| NetworkError::ResponseMissingField {
            context: "Relay quote response",
            field: "steps",
        })?;

    let rpc_url = config.rpc_url(NetworkId::Tempo);
    let provider = alloy::providers::RootProvider::<mpp::client::TempoNetwork>::new_http(rpc_url);

    let mut last_check: Option<(String, String)> = None;

    for step in steps {
        let kind = step
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        if kind != "transaction" {
            return Err(PaymentError::ChallengeSchema {
                context: "Relay bridge step",
                reason: format!("unsupported step kind: {kind}"),
            }
            .into());
        }

        let items = step
            .get("items")
            .and_then(|v| v.as_array())
            .ok_or_else(|| NetworkError::ResponseMissingField {
                context: "Relay bridge step",
                field: "items",
            })?;

        // Batch all items within a step into a single 0x76 transaction.
        // Relay steps typically contain approve + deposit as separate items
        // that must execute atomically (approve must be visible when deposit
        // calls transferFrom). This mirrors how session payments batch
        // approve + escrow.open into one tx.
        let mut calls = Vec::new();
        for item in items {
            let data = item
                .get("data")
                .ok_or_else(|| NetworkError::ResponseMissingField {
                    context: "Relay bridge step item",
                    field: "data",
                })?;

            calls.push(parse_relay_call(data)?);

            // Capture check endpoint if present (typically on the deposit step)
            if let Some(check) = item.get("check") {
                if let (Some(endpoint), Some(method)) = (check.get("endpoint"), check.get("method"))
                {
                    if let (Some(ep), Some(m)) = (endpoint.as_str(), method.as_str()) {
                        let full_url = if ep.starts_with("http") {
                            ep.to_string()
                        } else {
                            format!("{RELAY_API}{ep}")
                        };
                        last_check = Some((full_url, m.to_uppercase()));
                    }
                }
            }
        }

        if !calls.is_empty() {
            submit_tempo_tx(
                &provider,
                signer,
                TEMPO_CHAIN_ID,
                USDCE_TOKEN,
                signer.from,
                calls,
            )
            .await?;
        }
    }

    // Poll for bridge completion using the last check endpoint
    if let Some((check_url, _method)) = last_check {
        poll_bridge_status(&reqwest::Client::new(), &check_url).await?;
    }

    Ok(())
}

/// Parse a Relay step item's `data` object into a Tempo `Call`.
fn parse_relay_call(
    data: &serde_json::Value,
) -> Result<tempo_primitives::transaction::Call, TempoError> {
    let to_str = data.get("to").and_then(|v| v.as_str()).ok_or_else(|| {
        NetworkError::ResponseMissingField {
            context: "Relay tx data",
            field: "to",
        }
    })?;
    let to: Address = to_str.parse().map_err(|_| PaymentError::ChallengeSchema {
        context: "Relay tx data",
        reason: format!("invalid 'to' address: {to_str}"),
    })?;

    let input_hex = data.get("data").and_then(|v| v.as_str()).unwrap_or("0x");
    let input_bytes =
        hex::decode(input_hex.strip_prefix("0x").unwrap_or(input_hex)).map_err(|_| {
            PaymentError::ChallengeSchema {
                context: "Relay tx data",
                reason: "invalid 'data' hex".to_string(),
            }
        })?;

    let value = data.get("value").and_then(|v| v.as_str()).unwrap_or("0");
    let value_u256 = U256::from_str_radix(value.strip_prefix("0x").unwrap_or(value), 16)
        .or_else(|_| value.parse::<U256>())
        .unwrap_or(U256::ZERO);

    Ok(tempo_primitives::transaction::Call {
        to: TxKind::Call(to),
        value: value_u256,
        input: Bytes::from(input_bytes),
    })
}

/// Poll Relay status endpoint until bridge is complete.
async fn poll_bridge_status(
    http_client: &reqwest::Client,
    check_url: &str,
) -> Result<(), TempoError> {
    for _ in 0..MAX_POLL_ATTEMPTS {
        tokio::time::sleep(POLL_INTERVAL).await;

        let resp = http_client
            .get(check_url)
            .send()
            .await
            .map_err(NetworkError::Reqwest)?;

        let body: serde_json::Value = resp.json().await.map_err(NetworkError::Reqwest)?;

        let status = body
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        match status {
            "success" => return Ok(()),
            "failure" | "refunded" => {
                return Err(PaymentError::ChallengeSchema {
                    context: "Relay bridge",
                    reason: format!("bridge {status}"),
                }
                .into());
            }
            _ => continue,
        }
    }

    Err(PaymentError::ChallengeSchema {
        context: "Relay bridge",
        reason: format!("status polling timed out after {MAX_POLL_ATTEMPTS} attempts"),
    }
    .into())
}
