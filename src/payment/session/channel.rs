//! On-chain channel queries and scanning.
//!
//! Functions for querying channel state from the escrow contract,
//! scanning `ChannelOpened` events, and building channel-open transactions.

use alloy::eips::BlockNumberOrTag;
use alloy::primitives::{Address, Bytes, TxKind, B256, U256};
use alloy::providers::Provider;
use alloy::rpc::types::{Filter, TransactionRequest};
use alloy::sol;
use alloy::sol_types::SolCall;
use anyhow::{Context, Result};
use tempo_primitives::transaction::Call;

use crate::config::Config;
use crate::network::Network;
use crate::wallet::credentials::WalletCredentials;

// ==================== ABI Definitions ====================

sol! {
    interface ITIP20 {
        function approve(address spender, uint256 amount) external returns (bool);
    }
    interface IEscrow {
        function getChannel(bytes32 channelId) external view returns (
            address payer,
            address payee,
            address token,
            address authorizedSigner,
            uint128 deposit,
            uint128 settled,
            uint64 closeRequestedAt,
            bool finalized
        );
        function open(
            address payee,
            address token,
            uint128 deposit,
            bytes32 salt,
            address authorizedSigner
        ) external;
        function CLOSE_GRACE_PERIOD() external view returns (uint64 period);
    }
}

// ==================== Types ====================

/// On-chain channel state returned by recovery functions.
pub struct OnChainChannel {
    pub token: Address,
    pub deposit: u128,
    pub settled: u128,
    pub close_requested_at: u64,
}

/// Discovered on-chain channel with decoded metadata.
pub struct DiscoveredChannel {
    pub network: String,
    pub channel_id: String,
    pub escrow_contract: String,
    pub token: String,
    pub deposit: u128,
    pub settled: u128,
    pub close_requested_at: u64,
}

// ==================== Constants ====================

/// Maximum block range per `eth_getLogs` query (RPC limit).
const LOG_QUERY_BLOCK_RANGE: u64 = 50_000;

/// How far back (in blocks) to scan for `ChannelOpened` events.
/// At ~2s per block this covers ~2.3 days of history.
const LOG_SCAN_DEPTH: u64 = 100_000;

/// Safety margin subtracted from `get_block_number()` to avoid
/// "block range extends beyond current head" RPC errors caused by
/// indexing lag between the node's block tip and its log index.
const LOG_HEAD_MARGIN: u64 = 10;

/// keccak256("ChannelOpened(bytes32,address,address,address,address,bytes32,uint256)")
const CHANNEL_OPENED_TOPIC: &str =
    "0xcd6e60364f8ee4c2b0d62afc07a1fb04fd267ce94693f93f8f85daaa099b5c94";

// ==================== Helpers ====================

/// Check if an RPC error indicates the block range was too large.
fn is_rpc_range_error(err: &str) -> bool {
    err.contains("query returned more than")
        || err.contains("block range")
        || err.contains("too many")
        || err.contains("exceeds max")
        || err.contains("Log response size exceeded")
}

// ==================== Channel Queries ====================

/// Query the escrow contract for a specific channel's state.
///
/// Returns `Ok(None)` if `deposit == 0` or `finalized == true` (channel
/// does not exist or is already settled). Returns `Err` on RPC failures
/// so callers can distinguish "no channel" from "network error".
pub async fn get_channel_on_chain(
    provider: &alloy::providers::RootProvider<alloy::network::Ethereum>,
    escrow_contract: Address,
    channel_id: B256,
) -> Result<Option<OnChainChannel>> {
    let call_data = IEscrow::getChannelCall {
        channelId: channel_id,
    }
    .abi_encode();

    let tx = TransactionRequest::default()
        .to(escrow_contract)
        .input(Bytes::from(call_data).into());

    let result = provider
        .call(tx)
        .await
        .context("Failed to call getChannel on escrow contract")?;
    let decoded = IEscrow::getChannelCall::abi_decode_returns(&result)
        .context("Failed to decode getChannel response")?;

    if decoded.deposit == 0 || decoded.finalized {
        return Ok(None);
    }

    Ok(Some(OnChainChannel {
        token: decoded.token,
        deposit: decoded.deposit,
        settled: decoded.settled,
        close_requested_at: decoded.closeRequestedAt,
    }))
}

// ==================== Network Resolution ====================

/// Resolve networks to scan. If a specific network is given, use it.
/// Otherwise, derive from wallet credentials (all unique networks the user has keys for).
/// Falls back to Tempo mainnet if no credentials are available.
pub fn resolve_scan_networks(network_filter: Option<&str>) -> Vec<Network> {
    if let Some(name) = network_filter {
        return name.parse::<Network>().ok().into_iter().collect();
    }
    if let Ok(creds) = WalletCredentials::load() {
        let networks: Vec<Network> = creds
            .keys
            .iter()
            .filter_map(|k| Network::from_chain_id(k.chain_id))
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        if !networks.is_empty() {
            return networks;
        }
    }
    vec![Network::Tempo]
}

// ==================== Event Scanning ====================

/// Scan all known networks for open channels where `payer` is the sender.
///
/// This scans `ChannelOpened` events on each network's default escrow contract
/// filtered only by payer (not payee), so it finds *all* channels for this wallet.
pub async fn find_all_channels_for_payer(
    config: &Config,
    payer: Address,
    network_name: Option<&str>,
) -> Vec<DiscoveredChannel> {
    let networks = resolve_scan_networks(network_name);

    let event_topic: B256 = match CHANNEL_OPENED_TOPIC.parse() {
        Ok(t) => t,
        Err(e) => {
            tracing::error!(%e, "Invalid CHANNEL_OPENED_TOPIC constant");
            return Vec::new();
        }
    };
    let payer_topic = B256::left_padding_from(&payer.0 .0);

    let mut results = Vec::new();

    for network in &networks {
        let network_info = match config.resolve_network(network.as_str()) {
            Ok(info) => info,
            Err(_) => continue,
        };
        let rpc_url: url::Url = match network_info.rpc_url.parse() {
            Ok(u) => u,
            Err(_) => continue,
        };
        let provider = alloy::providers::RootProvider::new_http(rpc_url);

        let escrow: Address = match network.escrow_contract().parse() {
            Ok(a) => a,
            Err(_) => continue,
        };

        let latest = match provider.get_block_number().await {
            Ok(n) => n.saturating_sub(LOG_HEAD_MARGIN),
            Err(e) => {
                tracing::warn!(network = network.as_str(), %e, "failed to get block number, skipping network");
                continue;
            }
        };
        let earliest = latest.saturating_sub(LOG_SCAN_DEPTH);

        tracing::debug!(
            network = network.as_str(),
            latest,
            earliest,
            "scanning blocks for ChannelOpened events"
        );

        let mut chunk_end = latest;
        while chunk_end > earliest {
            let chunk_start = chunk_end
                .saturating_sub(LOG_QUERY_BLOCK_RANGE)
                .max(earliest);

            let filter = Filter::new()
                .address(escrow)
                .event_signature(event_topic)
                .topic2(payer_topic)
                .from_block(BlockNumberOrTag::Number(chunk_start))
                .to_block(BlockNumberOrTag::Number(chunk_end));

            let logs = match provider.get_logs(&filter).await {
                Ok(logs) => logs,
                Err(e) => {
                    let err_str = e.to_string();
                    if is_rpc_range_error(&err_str) && (chunk_end - chunk_start) > 1000 {
                        let halved = (chunk_end - chunk_start) / 2;
                        tracing::debug!(
                            network = network.as_str(),
                            old_range = chunk_end - chunk_start,
                            new_range = halved,
                            "RPC range too large, halving"
                        );
                        chunk_end = chunk_start + halved;
                        continue;
                    }
                    tracing::warn!(
                        network = network.as_str(),
                        chunk_start,
                        chunk_end,
                        %e,
                        "failed to query logs, skipping block range"
                    );
                    if chunk_start == earliest {
                        break;
                    }
                    chunk_end = chunk_start.saturating_sub(1);
                    continue;
                }
            };

            for log in &logs {
                let topics = log.topics();
                if topics.len() < 4 {
                    continue;
                }
                let channel_id = topics[1];

                // ChannelOpened event non-indexed data layout (ABI-encoded):
                //   [0..32]   address token       (left-padded, address at bytes 12..32)
                //   [32..64]  uint256 deposit
                //   [64..96]  bytes32 salt
                let data = log.data().data.as_ref();
                if data.len() < 96 {
                    continue;
                }
                let log_token = Address::from_slice(&data[12..32]);

                let on_chain = match get_channel_on_chain(&provider, escrow, channel_id).await {
                    Ok(Some(ch)) => ch,
                    Ok(None) => continue,
                    Err(e) => {
                        tracing::warn!(
                            network = network.as_str(),
                            %channel_id,
                            %e,
                            "failed to query channel state, skipping"
                        );
                        continue;
                    }
                };

                let token_str = format!("{:#x}", log_token);

                // Skip if we already found this channel_id
                let cid_str = format!("{:#x}", channel_id);
                if results
                    .iter()
                    .any(|r: &DiscoveredChannel| r.channel_id == cid_str)
                {
                    continue;
                }

                results.push(DiscoveredChannel {
                    network: network.as_str().to_string(),
                    channel_id: cid_str,
                    escrow_contract: format!("{:#x}", escrow),
                    token: token_str,
                    deposit: on_chain.deposit,
                    settled: on_chain.settled,
                    close_requested_at: on_chain.close_requested_at,
                });
            }

            if chunk_start == earliest {
                break;
            }
            chunk_end = chunk_start.saturating_sub(1);
        }
    }

    results
}

// ==================== Channel Helpers ====================

/// Build the escrow open calls: approve + open.
///
/// Constructs a 2-call sequence:
/// 1. `approve(escrow_contract, deposit)` on the currency token
/// 2. `IEscrow::open(payee, currency, deposit, salt, authorizedSigner)` on the escrow contract
pub(super) fn build_open_calls(
    currency: Address,
    escrow_contract: Address,
    deposit: u128,
    payee: Address,
    salt: B256,
    authorized_signer: Address,
) -> Vec<Call> {
    let approve_data = Bytes::from(
        ITIP20::approveCall {
            spender: escrow_contract,
            amount: U256::from(deposit),
        }
        .abi_encode(),
    );
    let open_data = Bytes::from(
        IEscrow::openCall::new((payee, currency, deposit, salt, authorized_signer)).abi_encode(),
    );

    vec![
        Call {
            to: TxKind::Call(currency),
            value: U256::ZERO,
            input: approve_data,
        },
        Call {
            to: TxKind::Call(escrow_contract),
            value: U256::ZERO,
            input: open_data,
        },
    ]
}

/// Read CLOSE_GRACE_PERIOD from the escrow contract. Returns None on error.
pub async fn read_grace_period(
    provider: &alloy::providers::RootProvider<alloy::network::Ethereum>,
    escrow_contract: Address,
) -> Option<u64> {
    let call_data = IEscrow::CLOSE_GRACE_PERIODCall {}.abi_encode();
    let tx = TransactionRequest::default()
        .to(escrow_contract)
        .input(Bytes::from(call_data).into());

    let result = provider.call(tx).await.ok()?;
    let decoded: u64 = IEscrow::CLOSE_GRACE_PERIODCall::abi_decode_returns(&result).ok()?;
    Some(decoded)
}

/// Query on-chain state for a channel by its hex ID and network name.
///
/// Returns `Ok(Some((token_address, deposit, settled)))` if the channel
/// exists on-chain, `Ok(None)` if confirmed not found (deposit == 0 or finalized),
/// or `Err` on RPC/config errors (callers should NOT treat errors as "missing").
pub async fn query_channel_state(
    config: &Config,
    channel_id_hex: &str,
    network_name: &str,
) -> Result<Option<(String, u128, u128)>> {
    let channel_id: B256 = channel_id_hex.parse().context("Invalid channel ID")?;
    let network: Network = network_name
        .parse()
        .map_err(|_| anyhow::anyhow!("Unknown network: {}", network_name))?;
    let network_info = config.resolve_network(network.as_str())?;
    let rpc_url: url::Url = network_info.rpc_url.parse().context("Invalid RPC URL")?;
    let provider = alloy::providers::RootProvider::new_http(rpc_url);
    let escrow: Address = network
        .escrow_contract()
        .parse()
        .context("Invalid escrow address")?;

    let on_chain = match get_channel_on_chain(&provider, escrow, channel_id).await? {
        Some(ch) => ch,
        None => return Ok(None),
    };

    Ok(Some((
        format!("{:#x}", on_chain.token),
        on_chain.deposit,
        on_chain.settled,
    )))
}
