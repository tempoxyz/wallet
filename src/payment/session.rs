//! Session-based payment handling.
//!
//! This module handles session payments (intent="session") using presto's
//! keychain-aware transaction building. Sessions open a payment channel
//! on-chain and then exchange off-chain vouchers for each request or SSE
//! token, settling on-chain when the session is closed.
//!
//! Sessions are persisted across CLI invocations via `session_store`. A
//! returning request to the same origin will reuse an existing channel
//! (skipping the on-chain open) and simply increment the cumulative
//! voucher amount.
//!
//! Unlike the mpp `TempoSessionProvider` (which only supports direct EOA
//! signing), this implementation uses presto's transaction builder to
//! support smart wallet / access key (keychain) signing mode.

use alloy::primitives::{Address, B256};
use anyhow::{Context, Result};
use futures::StreamExt;
use std::io::Write;
use std::str::FromStr;

use mpp::protocol::core::extract_tx_hash;
use mpp::protocol::methods::tempo::session::{SessionCredentialPayload, TempoSessionExt};
use mpp::protocol::methods::tempo::{compute_channel_id, sign_voucher};
use mpp::server::sse::{parse_event, SseEvent};
use mpp::{parse_receipt, parse_www_authenticate, ChallengeEcho};

use crate::config::Config;
use crate::error::map_mpp_validation_error;
use crate::http::{HttpResponse, RequestContext};
use crate::network::Network;
use crate::payment::session_store::{self, SessionRecord, SESSION_TTL_SECS};
use crate::wallet::signer::load_wallet_signer;

// ==================== Types ====================

/// Outcome of an on-chain close attempt.
pub enum CloseOutcome {
    /// Channel fully closed (withdrawn or cooperatively settled).
    Closed,
    /// `requestClose()` submitted or already pending; waiting for grace period.
    Pending { remaining_secs: u64 },
}

/// Result of a session request — either streamed (already printed) or a buffered response.
pub enum SessionResult {
    /// SSE tokens were streamed directly to stdout.
    Streamed { channel_id: String },
    /// A normal (non-SSE) response that should be handled by the regular output path.
    Response {
        response: HttpResponse,
        channel_id: String,
    },
}

/// State for an active session channel.
struct SessionState {
    channel_id: B256,
    escrow_contract: Address,
    chain_id: u64,
    cumulative_amount: u128,
}

/// Shared context for session operations (streaming, closing).
struct SessionContext<'a> {
    signer: &'a alloy::signers::local::PrivateKeySigner,
    echo: &'a ChallengeEcho,
    did: &'a str,
    request_ctx: &'a RequestContext,
    url: &'a str,
    network_name: &'a str,
    origin: &'a str,
    tick_cost: u128,
    deposit: u128,
    salt: String,
    recipient: String,
    currency: String,
}

impl SessionContext<'_> {
    /// Resolve the token symbol for the current session (e.g., "USDC" or "pathUSD").
    fn token_symbol(&self) -> &'static str {
        self.network_name
            .parse::<Network>()
            .ok()
            .and_then(|n| n.token_config_by_address(&self.currency))
            .map(|t| t.symbol)
            .unwrap_or("tokens")
    }
}

// ==================== On-Chain Recovery ====================

/// On-chain channel state returned by recovery functions.
struct OnChainChannel {
    channel_id: B256,
    salt: B256,
    token: Address,
    authorized_signer: Address,
    deposit: u128,
    settled: u128,
    close_requested_at: u64,
}

/// Query the escrow contract for a specific channel's state.
///
/// Returns `Ok(None)` if `deposit == 0` or `finalized == true` (channel
/// does not exist or is already settled). Returns `Err` on RPC failures
/// so callers can distinguish "no channel" from "network error".
async fn get_channel_on_chain(
    provider: &alloy::providers::RootProvider<alloy::network::Ethereum>,
    escrow_contract: Address,
    channel_id: B256,
    salt: B256,
) -> Result<Option<OnChainChannel>> {
    use alloy::providers::Provider;
    use alloy::sol;
    use alloy::sol_types::SolCall;

    sol! {
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
        }
    }

    let call_data = IEscrow::getChannelCall {
        channelId: channel_id,
    }
    .abi_encode();

    let tx = alloy::rpc::types::TransactionRequest::default()
        .to(escrow_contract)
        .input(alloy::primitives::Bytes::from(call_data).into());

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
        channel_id,
        salt,
        token: decoded.token,
        authorized_signer: decoded.authorizedSigner,
        deposit: decoded.deposit,
        settled: decoded.settled,
        close_requested_at: decoded.closeRequestedAt,
    }))
}

/// Maximum block range per `eth_getLogs` query (RPC limit).
const LOG_QUERY_BLOCK_RANGE: u64 = 50_000;

/// How far back (in blocks) to scan for `ChannelOpened` events.
/// At ~2s per block this covers ~2.3 days of history.
const LOG_SCAN_DEPTH: u64 = 100_000;

/// Safety margin subtracted from `get_block_number()` to avoid
/// "block range extends beyond current head" RPC errors caused by
/// indexing lag between the node's block tip and its log index.
const LOG_HEAD_MARGIN: u64 = 10;

/// Discovered on-chain channel with decoded metadata.
pub struct DiscoveredChannel {
    pub network: String,
    pub channel_id: String,
    pub escrow_contract: String,
    pub payee: String,
    pub authorized_signer: String,
    pub token: String,
    pub deposit: u128,
    pub settled: u128,
    pub close_requested_at: u64,
}

/// Scan all known networks for open channels where `payer` is the sender.
///
/// This scans `ChannelOpened` events on each network's default escrow contract
/// filtered only by payer (not payee), so it finds *all* channels for this wallet.
pub async fn find_all_channels_for_payer(
    config: &Config,
    payer: Address,
    network_name: Option<&str>,
) -> Vec<DiscoveredChannel> {
    use alloy::eips::BlockNumberOrTag;
    use alloy::primitives::FixedBytes;
    use alloy::providers::Provider;
    use alloy::rpc::types::Filter;

    let networks: Vec<Network> = if let Some(name) = network_name {
        name.parse::<Network>().ok().into_iter().collect()
    } else {
        Network::all().to_vec()
    };

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
        let provider =
            alloy::providers::RootProvider::<alloy::network::Ethereum>::new_http(rpc_url);

        let escrow: Address = match network.escrow_contract().parse() {
            Ok(a) => a,
            Err(_) => continue,
        };

        let event_topic: FixedBytes<32> =
            match "0xcd6e60364f8ee4c2b0d62afc07a1fb04fd267ce94693f93f8f85daaa099b5c94".parse() {
                Ok(t) => t,
                Err(_) => continue,
            };

        let payer_topic = B256::left_padding_from(&payer.0 .0);

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
                    let is_range_error = err_str.contains("query returned more than")
                        || err_str.contains("block range")
                        || err_str.contains("too many")
                        || err_str.contains("exceeds max")
                        || err_str.contains("Log response size exceeded");
                    if is_range_error && (chunk_end - chunk_start) > 1000 {
                        // Halve the range and retry from the same chunk_end
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

            {
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
                    let log_salt = B256::from_slice(&data[64..96]);

                    let on_chain =
                        match get_channel_on_chain(&provider, escrow, channel_id, log_salt).await {
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

                    let payee_addr = Address::from_slice(&topics[3].as_slice()[12..]);
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
                        payee: format!("{:#x}", payee_addr),
                        authorized_signer: format!("{:#x}", on_chain.authorized_signer),
                        token: token_str,
                        deposit: on_chain.deposit,
                        settled: on_chain.settled,
                        close_requested_at: on_chain.close_requested_at,
                    });
                }
            }

            if chunk_start == earliest {
                break;
            }
            chunk_end = chunk_start.saturating_sub(1);
        }
    }

    results
}

/// Scan on-chain `ChannelOpened` events to find an open channel matching
/// the given payer, payee, currency, and authorized signer.
///
/// Queries the escrow contract's event logs filtered by payer and payee,
/// walking backwards from the latest block in chunks to stay within RPC
/// block-range limits. Returns the most recently opened matching channel
/// that is still live on-chain.
async fn find_channel_on_chain(
    escrow_contract: Address,
    payer: Address,
    payee: Address,
    currency: Address,
    authorized_signer: Address,
    network_name: &str,
    config: &Config,
) -> Option<OnChainChannel> {
    use alloy::eips::BlockNumberOrTag;
    use alloy::primitives::FixedBytes;
    use alloy::providers::Provider;
    use alloy::rpc::types::Filter;

    let network_info = config.resolve_network(network_name).ok()?;
    let rpc_url: url::Url = network_info.rpc_url.parse().ok()?;
    let provider = alloy::providers::RootProvider::<alloy::network::Ethereum>::new_http(rpc_url);

    // keccak256("ChannelOpened(bytes32,address,address,address,address,bytes32,uint256)")
    let event_topic: FixedBytes<32> =
        "0xcd6e60364f8ee4c2b0d62afc07a1fb04fd267ce94693f93f8f85daaa099b5c94"
            .parse()
            .ok()?;

    // Event topics: [0]=sig, [1]=channelId, [2]=payer, [3]=payee
    let payer_topic = B256::left_padding_from(&payer.0 .0);
    let payee_topic = B256::left_padding_from(&payee.0 .0);

    let latest = provider
        .get_block_number()
        .await
        .ok()?
        .saturating_sub(LOG_HEAD_MARGIN);
    let earliest = latest.saturating_sub(LOG_SCAN_DEPTH);

    // Walk backwards in chunks so we find the most recent match first.
    let mut chunk_end = latest;
    while chunk_end > earliest {
        let chunk_start = chunk_end
            .saturating_sub(LOG_QUERY_BLOCK_RANGE)
            .max(earliest);

        let filter = Filter::new()
            .address(escrow_contract)
            .event_signature(event_topic)
            .topic2(payer_topic)
            .topic3(payee_topic)
            .from_block(BlockNumberOrTag::Number(chunk_start))
            .to_block(BlockNumberOrTag::Number(chunk_end));

        let logs = match provider.get_logs(&filter).await {
            Ok(logs) => logs,
            Err(e) => {
                let err_str = e.to_string();
                let is_range_error = err_str.contains("query returned more than")
                    || err_str.contains("block range")
                    || err_str.contains("too many")
                    || err_str.contains("exceeds max")
                    || err_str.contains("Log response size exceeded");
                if is_range_error && (chunk_end - chunk_start) > 1000 {
                    let halved = (chunk_end - chunk_start) / 2;
                    tracing::debug!(
                        old_range = chunk_end - chunk_start,
                        new_range = halved,
                        "RPC range too large, halving"
                    );
                    chunk_end = chunk_start + halved;
                    continue;
                }
                if chunk_start == earliest {
                    break;
                }
                chunk_end = chunk_start.saturating_sub(1);
                continue;
            }
        };

        // Most recent log last in results; walk in reverse.
        for log in logs.iter().rev() {
            // topic[0] = event sig, topic[1] = channelId, topic[2] = payer, topic[3] = payee
            let topics = log.topics();
            if topics.len() < 4 {
                continue;
            }
            let channel_id = topics[1];

            // Decode non-indexed data: (address token, address authorizedSigner, bytes32 salt, uint256 deposit)
            let data = log.data().data.as_ref();
            if data.len() < 128 {
                continue;
            }
            let log_token = Address::from_slice(&data[12..32]);
            let log_signer = Address::from_slice(&data[44..64]);
            let log_salt = B256::from_slice(&data[64..96]);

            if log_token != currency || log_signer != authorized_signer {
                continue;
            }

            // Verify channel is still open on-chain.
            match get_channel_on_chain(&provider, escrow_contract, channel_id, log_salt).await {
                Ok(Some(ch)) => return Some(ch),
                Ok(None) => continue,
                Err(e) => {
                    tracing::warn!(error = %e, "failed to verify channel on-chain, skipping");
                    continue;
                }
            }
        }

        if chunk_start == earliest {
            break;
        }
        chunk_end = chunk_start.saturating_sub(1);
    }

    None
}

// ==================== Channel Helpers ====================

/// Extract the origin (scheme://host\[:port\]) from a URL.
fn extract_origin(url: &str) -> String {
    match url::Url::parse(url) {
        Ok(parsed) => {
            let scheme = parsed.scheme();
            let host = parsed.host_str().unwrap_or("unknown");
            match parsed.port() {
                Some(port) => format!("{scheme}://{host}:{port}"),
                None => format!("{scheme}://{host}"),
            }
        }
        Err(_) => url.to_string(),
    }
}

/// Build the escrow open calls: approve + open.
///
/// Constructs a 2-call sequence:
/// 1. `approve(escrow_contract, deposit)` on the currency token
/// 2. `IEscrow::open(payee, currency, deposit, salt, authorizedSigner)` on the escrow contract
fn build_open_calls(
    currency: Address,
    escrow_contract: Address,
    deposit: u128,
    payee: Address,
    salt: B256,
    authorized_signer: Address,
) -> Vec<tempo_primitives::transaction::Call> {
    use alloy::primitives::{Bytes, TxKind, U256};
    use alloy::sol;
    use alloy::sol_types::SolCall;
    use tempo_primitives::transaction::Call;

    sol! {
        interface ITIP20 {
            function approve(address spender, uint256 amount) external returns (bool);
        }
        interface IEscrow {
            function open(
                address payee,
                address token,
                uint128 deposit,
                bytes32 salt,
                address authorizedSigner
            ) external;
        }
    }

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

/// Send the actual request with a voucher and handle the response.
async fn send_session_request(
    ctx: &SessionContext<'_>,
    state: &mut SessionState,
) -> Result<SessionResult> {
    if ctx.request_ctx.log_enabled() {
        eprintln!("Sending request with session voucher...");
    }

    let voucher_credential = build_voucher_credential(ctx.signer, ctx.echo, ctx.did, state).await?;

    let voucher_auth = mpp::format_authorization(&voucher_credential)
        .context("Failed to format voucher credential")?;

    let data_request = ctx
        .request_ctx
        .build_reqwest_request(ctx.url, None)?
        .header("Authorization", &voucher_auth);

    let response = data_request
        .send()
        .await
        .context("Failed to send session request")?;

    let status = response.status();
    if status.as_u16() == 402 || status.is_client_error() || status.is_server_error() {
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!(
            "Session request failed: HTTP {} — {}",
            status,
            body.chars().take(500).collect::<String>()
        );
    }

    let is_sse = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.contains("text/event-stream"));

    if is_sse {
        stream_sse_response(ctx, state, response).await?;
        Ok(SessionResult::Streamed {
            channel_id: format!("{:#x}", state.channel_id),
        })
    } else {
        let status_code = status.as_u16() as u32;
        let mut headers = std::collections::HashMap::new();
        for (key, value) in response.headers() {
            if let Ok(value_str) = value.to_str() {
                headers.insert(key.as_str().to_lowercase(), value_str.to_string());
            }
        }
        let body = response.bytes().await?.to_vec();

        Ok(SessionResult::Response {
            response: HttpResponse {
                status_code,
                headers,
                body,
            },
            channel_id: format!("{:#x}", state.channel_id),
        })
    }
}

// ==================== Voucher ====================

/// Build a voucher credential for an existing session.
async fn build_voucher_credential(
    signer: &alloy::signers::local::PrivateKeySigner,
    echo: &ChallengeEcho,
    did: &str,
    state: &SessionState,
) -> Result<mpp::PaymentCredential> {
    let sig = sign_voucher(
        signer,
        state.channel_id,
        state.cumulative_amount,
        state.escrow_contract,
        state.chain_id,
    )
    .await
    .context("Failed to sign voucher")?;

    let payload = SessionCredentialPayload::Voucher {
        channel_id: format!("{}", state.channel_id),
        cumulative_amount: state.cumulative_amount.to_string(),
        signature: format!("0x{}", hex::encode(&sig)),
    };

    Ok(mpp::PaymentCredential::with_source(
        echo.clone(),
        did.to_string(),
        payload,
    ))
}

/// Post a voucher to the server in a background task.
///
/// We MUST NOT await the response inline because the server may respond
/// with a streaming body (treating the POST as a new chat request).
/// Awaiting would deadlock: the server waits for us to read the SSE
/// stream, and we wait for the POST response.
fn post_voucher(client: &reqwest::Client, url: &str, auth: &str, verbose: bool) {
    let vc = client.clone();
    let url = url.to_string();
    let auth = auth.to_string();
    tokio::spawn(async move {
        match vc.post(&url).header("Authorization", &auth).send().await {
            Ok(resp) => {
                if verbose {
                    let status = resp.status();
                    let ct = resp
                        .headers()
                        .get("content-type")
                        .and_then(|v| v.to_str().ok())
                        .unwrap_or("none")
                        .to_string();
                    eprintln!("[voucher POST: {} content-type={}]", status, ct);
                }
            }
            Err(e) => {
                eprintln!("[voucher POST failed: {}]", e);
            }
        }
    });
}

// ==================== SSE Streaming ====================

/// Stream SSE events from a response, handling voucher top-ups mid-stream.
///
/// Persists cumulative amount updates during streaming so that if the
/// process is interrupted, the session record reflects the last voucher sent.
///
/// The server has a known race condition where its `wait_for_update` notification
/// can be lost (tokio::sync::Notify without permit storage). When a voucher POST
/// arrives but the server hasn't started awaiting yet, the notification is dropped
/// and the stream stalls. We work around this by re-posting the same voucher if
/// no progress is seen within a short timeout after the last need-voucher event.
async fn stream_sse_response(
    ctx: &SessionContext<'_>,
    state: &mut SessionState,
    response: reqwest::Response,
) -> Result<()> {
    let runtime = &ctx.request_ctx.runtime;
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut token_count: u64 = 0;
    let mut stdout = std::io::stdout();

    let mut stream_done = false;

    // Cap SSE buffer to prevent unbounded growth from malformed streams
    // that never emit the \n\n event delimiter.
    const MAX_BUFFER_SIZE: usize = 4 * 1024 * 1024; // 4 MB

    // Reuse a single client for voucher POSTs to maintain connection affinity
    // with the server (important when behind a load balancer).
    let voucher_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_default();

    // Track pending voucher for retry on stall. When we send a voucher but
    // the server's notify is lost, we need to re-send to wake it up.
    let mut pending_voucher_auth: Option<String> = None;
    let mut voucher_retry_count: u32 = 0;

    // Constants for stream behavior.
    const MAX_VOUCHER_RETRIES: u32 = 5;
    const NORMAL_TIMEOUT_SECS: u64 = 30;
    const VOUCHER_STALL_TIMEOUT_SECS: u64 = 3;

    // Normal timeout for when we're actively receiving tokens.
    let normal_timeout = std::time::Duration::from_secs(NORMAL_TIMEOUT_SECS);
    // Short timeout after sending a voucher — if the server doesn't resume
    // quickly, the notify was likely lost and we should re-post.
    let base_stall_timeout = std::time::Duration::from_secs(VOUCHER_STALL_TIMEOUT_SECS);
    // Exponential backoff for re-posting the same voucher (caps at normal_timeout)
    let mut current_stall_timeout = base_stall_timeout;

    loop {
        if stream_done {
            break;
        }

        let timeout = if pending_voucher_auth.is_some() {
            current_stall_timeout
        } else {
            normal_timeout
        };

        let chunk = match tokio::time::timeout(timeout, stream.next()).await {
            Ok(Some(chunk)) => chunk,
            Ok(None) => break, // stream ended
            Err(_) => {
                // Timeout — if we have a pending voucher, re-post it
                if let Some(ref auth) = pending_voucher_auth {
                    voucher_retry_count += 1;
                    if voucher_retry_count > MAX_VOUCHER_RETRIES {
                        if runtime.debug_enabled() {
                            eprintln!(
                                "[stream stall — voucher not accepted after {} retries]",
                                MAX_VOUCHER_RETRIES
                            );
                        }
                        break;
                    }
                    if runtime.debug_enabled() {
                        eprintln!(
                            "[re-posting voucher (retry {}/{})]",
                            voucher_retry_count, MAX_VOUCHER_RETRIES
                        );
                    }
                    let verbose = runtime.debug_enabled();
                    post_voucher(&voucher_client, ctx.url, auth, verbose);
                    // Backoff the stall timeout for the next retry, up to the normal timeout
                    current_stall_timeout =
                        std::cmp::min(current_stall_timeout.saturating_mul(2), normal_timeout);
                    continue;
                }
                if runtime.debug_enabled() {
                    eprintln!(
                        "[stream timeout — no data for {}s]",
                        normal_timeout.as_secs()
                    );
                }
                break;
            }
        };
        let chunk = chunk.context("Stream error")?;
        let chunk_str = String::from_utf8_lossy(&chunk);
        // Normalize \r\n to \n so SSE event boundary detection works with
        // servers/proxies that emit CRLF line endings.
        if chunk_str.contains('\r') {
            buffer.push_str(&chunk_str.replace("\r\n", "\n"));
        } else {
            buffer.push_str(&chunk_str);
        }

        if buffer.len() > MAX_BUFFER_SIZE {
            anyhow::bail!("SSE buffer exceeded {MAX_BUFFER_SIZE} bytes without a complete event — aborting stream");
        }

        while let Some(pos) = buffer.find("\n\n") {
            let event_str = buffer[..pos + 2].to_string();
            buffer = buffer[pos + 2..].to_string();

            if let Some(event) = parse_event(&event_str) {
                match event {
                    SseEvent::Message(data) => {
                        // Any message means the voucher was accepted
                        pending_voucher_auth = None;
                        voucher_retry_count = 0;
                        current_stall_timeout = base_stall_timeout;

                        if data.trim() == "[DONE]" {
                            stream_done = true;
                            break;
                        }
                        if let Some(content) = extract_sse_content(&data) {
                            token_count += 1;
                            write!(stdout, "{}", content)?;
                            stdout.flush()?;
                        }
                        // Detect OpenAI finish_reason to know stream is ending
                        if is_stream_finished(&data) {
                            stream_done = true;
                            break;
                        }
                    }
                    SseEvent::PaymentNeedVoucher(nv) => {
                        let required: u128 = nv.required_cumulative.parse().unwrap_or(0);
                        let deposit: u128 = nv.deposit.parse().unwrap_or(0);

                        // Authorize up to the full deposit so the server can
                        // stream multiple tokens before needing another voucher,
                        // instead of a network round-trip per token.
                        let voucher_amount = if deposit > 0 { deposit } else { required };

                        if runtime.debug_enabled() {
                            eprintln!(
                                "[voucher top-up: required={} authorizing={}]",
                                required, voucher_amount
                            );
                        }

                        state.cumulative_amount = voucher_amount;

                        // Persist the updated cumulative mid-stream
                        let _ = persist_session(ctx, state);

                        let voucher =
                            build_voucher_credential(ctx.signer, ctx.echo, ctx.did, state).await?;
                        let auth = mpp::format_authorization(&voucher)
                            .context("Failed to format voucher")?;

                        let verbose = runtime.debug_enabled();
                        post_voucher(&voucher_client, ctx.url, &auth, verbose);

                        // Track this voucher for retry if the server stalls
                        pending_voucher_auth = Some(auth);
                        voucher_retry_count = 0;
                        current_stall_timeout = base_stall_timeout;
                    }
                    SseEvent::PaymentReceipt(receipt) => {
                        pending_voucher_auth = None;
                        if runtime.log_enabled() {
                            eprintln!();
                            eprintln!("Stream receipt:");
                            eprintln!("  Channel: {}", receipt.channel_id);
                            eprintln!("  Spent: {}", receipt.spent);
                            if let Some(units) = receipt.units {
                                eprintln!("  Units: {}", units);
                            }
                            if let Some(ref tx) = receipt.tx_hash {
                                eprintln!("  TX: {}", tx);
                            }
                        }
                        // Receipt signals stream completion
                        stream_done = true;
                        break;
                    }
                }
            }
        }
    }

    writeln!(stdout)?;

    if runtime.log_enabled() {
        eprintln!("Tokens streamed: {}", token_count);
        let cumulative_f64 = state.cumulative_amount as f64 / 1e6;
        let symbol = ctx.token_symbol();
        eprintln!("Voucher cumulative: {cumulative_f64:.6} {symbol}");
    }

    Ok(())
}

/// Check if an OpenAI streaming chunk signals completion.
///
/// Returns true if the chunk contains `"finish_reason":"stop"` (or any non-null
/// finish_reason), indicating the model is done generating.
fn is_stream_finished(raw: &str) -> bool {
    let trimmed = raw.trim();
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Some(reason) = v
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("finish_reason"))
        {
            return !reason.is_null();
        }
    }
    false
}

/// Extract token content from an OpenAI chat completion chunk.
///
/// Handles the `data:` payload from SSE streams in OpenAI format:
/// `{"choices":[{"delta":{"content":"token"}}]}`.
/// Returns None for `[DONE]`, empty deltas, or unparseable data.
fn extract_sse_content(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed == "[DONE]" {
        return None;
    }
    // Try to parse as OpenAI chat completion chunk
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
        if let Some(content) = v
            .get("choices")
            .and_then(|c| c.get(0))
            .and_then(|c| c.get("delta"))
            .and_then(|d| d.get("content"))
            .and_then(|c| c.as_str())
        {
            if !content.is_empty() {
                return Some(content.to_string());
            }
            return None;
        }
        // No delta.content — could be role-only or finish delta
        return None;
    }
    // Not JSON — return raw content as-is (plain text SSE)
    Some(trimmed.to_string())
}

// ==================== Persistence ====================

/// Persist or update the session record to disk.
fn persist_session(ctx: &SessionContext<'_>, state: &SessionState) -> Result<()> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let echo_json =
        serde_json::to_string(ctx.echo).context("Failed to serialize challenge echo")?;

    let session_key = session_store::session_key(ctx.url);
    let existing = session_store::load_session(&session_key)?;

    let record = if let Some(mut rec) = existing {
        // Update existing record
        rec.set_cumulative_amount(state.cumulative_amount);
        rec.challenge_echo = echo_json;
        rec.touch();
        rec
    } else {
        SessionRecord {
            version: 1,
            origin: ctx.origin.to_string(),
            request_url: ctx.url.to_string(),
            network_name: ctx.network_name.to_string(),
            chain_id: state.chain_id,
            escrow_contract: format!("{:#x}", state.escrow_contract),
            currency: ctx.currency.clone(),
            recipient: ctx.recipient.clone(),
            payer: ctx.did.to_string(),
            authorized_signer: format!("{:#x}", ctx.signer.address()),
            salt: ctx.salt.clone(),
            channel_id: format!("{}", state.channel_id),
            deposit: ctx.deposit.to_string(),
            tick_cost: ctx.tick_cost.to_string(),
            cumulative_amount: state.cumulative_amount.to_string(),
            did: ctx.did.to_string(),
            challenge_echo: echo_json,
            challenge_id: ctx.echo.id.clone(),
            created_at: now,
            last_used_at: now,
            expires_at: now + SESSION_TTL_SECS,
        }
    };

    session_store::save_session(&record)?;

    if ctx.request_ctx.log_enabled() {
        let cumulative_f64 = state.cumulative_amount as f64 / 1e6;
        let symbol = ctx.token_symbol();
        eprintln!("Session persisted (cumulative: {cumulative_f64:.6} {symbol})");
    }

    Ok(())
}

// ==================== Main Logic ====================

/// Handle an MPP session flow (402 with intent="session").
///
/// This manages the session lifecycle with persistence:
/// 1. Parse the session challenge from the initial 402 response
/// 2. Check for an existing persisted session for this origin
/// 3. If found and not expired, reuse it (skip channel open)
/// 4. If not found or expired, open a new channel on-chain
/// 5. Send the real request with a voucher
/// 6. Stream SSE events (or return buffered response)
/// 7. Persist/update the session (do NOT close the channel)
pub async fn handle_session_request(
    config: &Config,
    request_ctx: &RequestContext,
    url: &str,
    initial_response: &crate::http::HttpResponse,
) -> Result<SessionResult> {
    let www_auth = initial_response
        .get_header("www-authenticate")
        .ok_or_else(|| crate::error::PrestoError::MissingHeader("WWW-Authenticate".to_string()))?;

    let challenge =
        parse_www_authenticate(www_auth).context("Failed to parse WWW-Authenticate header")?;

    challenge
        .validate_for_session("tempo")
        .map_err(|e| map_mpp_validation_error(e, &challenge))?;

    let session_req: mpp::SessionRequest = challenge
        .request
        .decode()
        .context("Failed to parse session request from challenge")?;

    let network = network_from_session_request(&session_req)
        .context("Failed to resolve network from session request")?;
    let network_name = network.as_str();

    let tick_cost: u128 = session_req
        .amount
        .parse()
        .context("Invalid session amount")?;

    let escrow_str = session_req
        .escrow_contract()
        .context("Missing escrow contract in session challenge")?;
    let escrow_contract: Address = escrow_str
        .parse()
        .context("Invalid escrow contract address")?;

    let chain_id = session_req
        .chain_id()
        .context("Missing chain ID in session challenge")?;

    let currency: Address = session_req
        .currency
        .parse()
        .context("Invalid currency address")?;

    let recipient: Address = session_req
        .recipient
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("Missing recipient in session challenge"))?
        .parse()
        .context("Invalid recipient address")?;

    // Resolve token metadata for human-readable amounts
    let token_config = network
        .token_config_by_address(&session_req.currency)
        .unwrap_or(crate::network::TokenConfig {
            symbol: "tokens",
            decimals: 6,
            address: "",
        });

    if request_ctx.log_enabled() {
        let cost_display = crate::cli::query::format_token_amount(
            tick_cost,
            token_config.symbol,
            token_config.decimals,
        );
        eprintln!(
            "Cost per {}: {}",
            session_req.unit_type.as_deref().unwrap_or("request"),
            cost_display
        );
    }

    // Dry-run: print session parameters and exit without signing or transacting
    if request_ctx.runtime.dry_run {
        let network_enum = crate::network::Network::from_str(network_name)
            .unwrap_or(crate::network::Network::Tempo);
        let explorer = network_enum.info().explorer;

        let cost_display = crate::cli::query::format_token_amount(
            tick_cost,
            token_config.symbol,
            token_config.decimals,
        );

        println!("[DRY RUN] Session payment would be made:");
        println!("Protocol: MPP (https://mpp.sh)");
        println!("Method: {}", challenge.method);
        println!("Intent: session");
        println!("Network: {}", network_name);
        println!(
            "Cost per {}: {}",
            session_req.unit_type.as_deref().unwrap_or("request"),
            cost_display
        );
        println!(
            "Currency: {}",
            crate::network::format_address_link(&session_req.currency, explorer.as_ref())
        );
        if let Some(ref recipient) = session_req.recipient {
            println!(
                "Recipient: {}",
                crate::network::format_address_link(recipient, explorer.as_ref())
            );
        }
        if let Some(ref deposit) = session_req.suggested_deposit {
            let deposit_val: u128 = deposit.parse().unwrap_or(0);
            let deposit_display = crate::cli::query::format_token_amount(
                deposit_val,
                token_config.symbol,
                token_config.decimals,
            );
            println!("Suggested deposit: {}", deposit_display);
        }

        return Ok(SessionResult::Response {
            response: crate::http::HttpResponse {
                status_code: 200,
                headers: std::collections::HashMap::new(),
                body: Vec::new(),
            },
            channel_id: String::new(),
        });
    }

    // Load signer and resolve signing mode (direct or keychain)
    let signing = load_wallet_signer(network_name)?;

    let key_address = signing.signer.address();
    let from = signing.from;

    // Always refresh the challenge echo from the current 402 response
    let echo = challenge.to_echo();
    let origin = extract_origin(url);
    let session_key = session_store::session_key(url);

    // Determine deposit: use suggested_deposit or default to 1 token (1_000_000 atomic units)
    let deposit: u128 = session_req
        .suggested_deposit
        .as_ref()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1_000_000);

    let did = format!("did:pkh:eip155:{}:{:#x}", chain_id, from);

    // Check for an existing persisted session.
    // Reuse requires matching payer AND challenge parameters (escrow, currency,
    // recipient, chain) to avoid a wasted round trip when the server changes config.
    let existing = session_store::load_session(&session_key)?;
    let reuse = existing.as_ref().is_some_and(|r| {
        !r.is_expired()
            && r.payer == did
            && r.escrow_contract == format!("{:#x}", escrow_contract)
            && r.currency == format!("{:#x}", currency)
            && r.recipient == format!("{:#x}", recipient)
            && r.chain_id == chain_id
    });

    if reuse {
        let record = existing.unwrap();
        if request_ctx.log_enabled() {
            eprintln!("Reusing existing session for {}", origin);
            eprintln!("  Channel: {}", record.channel_id);
        }

        let channel_id: B256 = record.channel_id_b256()?;

        let prev_cumulative: u128 = record.cumulative_amount_u128()?;

        let mut state = SessionState {
            channel_id,
            escrow_contract,
            chain_id,
            cumulative_amount: prev_cumulative + tick_cost,
        };

        let ctx = SessionContext {
            signer: &signing.signer,
            echo: &echo,
            did: &did,
            request_ctx,
            url,
            network_name,
            origin: &origin,
            tick_cost,
            deposit,
            salt: record.salt.clone(),
            recipient: format!("{:#x}", recipient),
            currency: format!("{:#x}", currency),
        };

        match send_session_request(&ctx, &mut state).await {
            Ok(result) => {
                persist_session(&ctx, &state)?;
                return Ok(result);
            }
            Err(e) => {
                // If the server rejected us (stale session), delete and fall through
                if request_ctx.log_enabled() {
                    eprintln!("Session reuse failed: {e}");
                    eprintln!("Opening new channel...");
                }
                session_store::delete_session(&session_key)?;
                // Fall through to open a new channel
            }
        }
    } else if let Some(ref record) = existing {
        // Expired or different payer — clean up
        if request_ctx.log_enabled() {
            if record.is_expired() {
                eprintln!("Existing session expired, opening new channel...");
            } else {
                eprintln!("Existing session for different payer, opening new channel...");
            }
        }
        session_store::delete_session(&session_key)?;
    }

    // === Try on-chain recovery by scanning ChannelOpened events ===
    {
        if request_ctx.log_enabled() {
            eprintln!("Checking for existing channel on-chain...");
        }
        if let Some(on_chain) = find_channel_on_chain(
            escrow_contract,
            from,
            recipient,
            currency,
            key_address,
            network_name,
            config,
        )
        .await
        {
            if request_ctx.log_enabled() {
                eprintln!("Recovered channel from on-chain state");
                eprintln!("  Channel: {:#x}", on_chain.channel_id);
            }

            let cumulative = on_chain.settled + tick_cost;
            let mut state = SessionState {
                channel_id: on_chain.channel_id,
                escrow_contract,
                chain_id,
                cumulative_amount: cumulative,
            };

            let ctx = SessionContext {
                signer: &signing.signer,
                echo: &echo,
                did: &did,
                request_ctx,
                url,
                network_name,
                origin: &origin,
                tick_cost,
                deposit: on_chain.deposit,
                salt: format!("{:#x}", on_chain.salt),
                recipient: format!("{:#x}", recipient),
                currency: format!("{:#x}", currency),
            };

            match send_session_request(&ctx, &mut state).await {
                Ok(result) => {
                    persist_session(&ctx, &state)?;
                    return Ok(result);
                }
                Err(e) => {
                    if request_ctx.log_enabled() {
                        eprintln!("Recovered channel failed: {e}");
                        eprintln!("Opening new channel...");
                    }
                    // Fall through to open new channel
                }
            }
        }
    }

    // === Open a new channel ===

    let salt = B256::random();
    let authorized_signer = key_address;
    let channel_id = compute_channel_id(
        from,
        recipient,
        currency,
        salt,
        authorized_signer,
        escrow_contract,
        chain_id,
    );

    if request_ctx.log_enabled() {
        let deposit_display = crate::cli::query::format_token_amount(
            deposit,
            token_config.symbol,
            token_config.decimals,
        );
        eprintln!("Opening payment channel...");
        eprintln!("  Deposit: {}", deposit_display);
        eprintln!("  Channel: {:#x}", channel_id);
    }

    let open_calls = build_open_calls(
        currency,
        escrow_contract,
        deposit,
        recipient,
        salt,
        authorized_signer,
    );

    let initial_cumulative = tick_cost;
    let voucher_sig = sign_voucher(
        &signing.signer,
        channel_id,
        initial_cumulative,
        escrow_contract,
        chain_id,
    )
    .await
    .context("Failed to sign initial voucher")?;

    let open_credential = create_tempo_payment_from_calls(
        config, &signing, &challenge, open_calls, currency, chain_id,
    )
    .await?;

    let open_tx = open_credential
        .payload
        .get("signature")
        .or_else(|| open_credential.payload.get("transaction"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing transaction in open credential"))?
        .to_string();

    let open_payload = SessionCredentialPayload::Open {
        payload_type: "transaction".to_string(),
        channel_id: format!("{}", channel_id),
        transaction: open_tx,
        authorized_signer: Some(format!("{:#x}", authorized_signer)),
        cumulative_amount: initial_cumulative.to_string(),
        signature: format!("0x{}", hex::encode(&voucher_sig)),
    };

    let session_credential =
        mpp::PaymentCredential::with_source(echo.clone(), did.clone(), open_payload);

    let auth_header = mpp::format_authorization(&session_credential)
        .context("Failed to format open credential")?;

    let open_headers = vec![("Authorization".to_string(), auth_header)];
    let open_response = request_ctx.execute(url, Some(&open_headers)).await?;

    // Retry on 410 "channel not funded" — the on-chain tx may still be confirming.
    let open_response = if open_response.status_code == 410 {
        let body = open_response.body_string().unwrap_or_default();
        if body.contains("channel not funded") || body.contains("Channel Not Found") {
            if request_ctx.log_enabled() {
                eprintln!("Channel tx still confirming, waiting to retry...");
            }
            let delays = [2000, 3000, 5000];
            let mut final_response = None;
            for delay_ms in delays {
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                let retry_headers = vec![("Authorization".to_string(), open_headers[0].1.clone())];
                let resp = request_ctx.execute(url, Some(&retry_headers)).await?;
                if resp.status_code < 400 {
                    final_response = Some(resp);
                    break;
                }
                let retry_body = resp.body_string().unwrap_or_default();
                if resp.status_code != 410 {
                    anyhow::bail!(
                        "Session open failed: HTTP {} — {}",
                        resp.status_code,
                        retry_body.chars().take(500).collect::<String>()
                    );
                }
            }
            match final_response {
                Some(resp) => resp,
                None => anyhow::bail!(
                    "Session open failed after retries: channel not funded on-chain. TX may have failed."
                ),
            }
        } else {
            anyhow::bail!(
                "Session open failed: HTTP 410 — {}",
                body.chars().take(500).collect::<String>()
            );
        }
    } else if open_response.status_code >= 400 {
        let body = open_response.body_string().unwrap_or_default();
        anyhow::bail!(
            "Session open failed: HTTP {} — {}",
            open_response.status_code,
            body.chars().take(500).collect::<String>()
        );
    } else {
        open_response
    };

    if let Some(receipt_str) = open_response.get_header("payment-receipt") {
        if let Ok(receipt) = parse_receipt(receipt_str) {
            let tx_ref = extract_tx_hash(receipt_str).unwrap_or(receipt.reference);
            let explorer = Network::from_str(network_name)
                .ok()
                .and_then(|n| n.info().explorer);
            if request_ctx.log_enabled() {
                if let Some(exp) = explorer.as_ref() {
                    let tx_url = exp.tx_url(&tx_ref);
                    eprintln!("Channel open tx: {}", tx_url);
                } else {
                    eprintln!("Channel open tx: {}", tx_ref);
                }
            }
        }
    }

    let mut state = SessionState {
        channel_id,
        escrow_contract,
        chain_id,
        cumulative_amount: initial_cumulative,
    };

    let ctx = SessionContext {
        signer: &signing.signer,
        echo: &echo,
        did: &did,
        request_ctx,
        url,
        network_name,
        origin: &origin,
        tick_cost,
        deposit,
        salt: format!("{}", salt),
        recipient: format!("{:#x}", recipient),
        currency: format!("{:#x}", currency),
    };

    let result = send_session_request(&ctx, &mut state).await?;
    persist_session(&ctx, &state)?;
    Ok(result)
}

/// Close a session from a persisted record.
///
/// Used by `presto session close` to send a close credential to the server.
pub async fn close_session_from_record(
    record: &session_store::SessionRecord,
    config: &Config,
) -> Result<CloseOutcome> {
    let echo: ChallengeEcho = serde_json::from_str(&record.challenge_echo)
        .context("Failed to parse persisted challenge echo")?;

    let wallet = load_wallet_signer(&record.network_name)?;

    let channel_id: B256 = record.channel_id_b256()?;

    let escrow_contract: Address = record
        .escrow_contract
        .parse()
        .context("Invalid escrow_contract in session record")?;

    let cumulative_amount: u128 = record.cumulative_amount_u128()?;

    let sig = sign_voucher(
        &wallet.signer,
        channel_id,
        cumulative_amount,
        escrow_contract,
        record.chain_id,
    )
    .await
    .context("Failed to sign close voucher")?;

    // Try cooperative close via the server first
    let server_result = try_server_close(record, &echo, channel_id, cumulative_amount, &sig).await;

    match server_result {
        Ok(()) => return Ok(CloseOutcome::Closed),
        Err(e) => {
            tracing::debug!(%e, "server close failed, falling back to on-chain close");
        }
    }

    // Fallback: payer-initiated close (requestClose → withdraw)
    close_on_chain(config, record, &wallet, channel_id, escrow_contract).await
}

/// Try cooperative close via the server.
async fn try_server_close(
    record: &session_store::SessionRecord,
    echo: &ChallengeEcho,
    channel_id: B256,
    cumulative_amount: u128,
    sig: &[u8],
) -> Result<()> {
    let close_payload = SessionCredentialPayload::Close {
        channel_id: format!("{}", channel_id),
        cumulative_amount: cumulative_amount.to_string(),
        signature: format!("0x{}", hex::encode(sig)),
    };

    let credential =
        mpp::PaymentCredential::with_source(echo.clone(), record.did.clone(), close_payload);

    let auth =
        mpp::format_authorization(&credential).context("Failed to format close credential")?;

    let client = reqwest::Client::builder()
        .build()
        .context("Failed to build HTTP client")?;

    let close_url = if record.request_url.is_empty() {
        format!(
            "{}://{}",
            if record.origin.starts_with("https") {
                "https"
            } else {
                "http"
            },
            record.origin.split("://").nth(1).unwrap_or(&record.origin),
        )
    } else {
        record.request_url.clone()
    };

    let response = client
        .post(&close_url)
        .header("Authorization", &auth)
        .send()
        .await
        .context("Channel close request failed")?;

    let status = response.status();

    // HTTP 410 Gone means the channel is already finalized on-chain.
    // Treat this as a successful close — the local record just needs cleanup.
    if status == reqwest::StatusCode::GONE {
        return Ok(());
    }

    if status.is_client_error() || status.is_server_error() {
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| String::from("<no body>"));
        let reason = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| {
                v.get("error")
                    .or_else(|| v.get("message"))
                    .or_else(|| v.get("detail"))
                    .and_then(|s| s.as_str().map(String::from))
            })
            .unwrap_or_else(|| body.chars().take(200).collect());
        anyhow::bail!(
            "Channel close rejected: HTTP {} — {}",
            status.as_u16(),
            reason
        );
    }

    if let Some(receipt_str) = response.headers().get("payment-receipt") {
        if let Ok(receipt_str) = receipt_str.to_str() {
            if let Ok(receipt) = parse_receipt(receipt_str) {
                let tx_ref = extract_tx_hash(receipt_str).unwrap_or(receipt.reference);
                let explorer = Network::from_str(&record.network_name)
                    .ok()
                    .and_then(|n| n.info().explorer);
                if let Some(exp) = explorer.as_ref() {
                    let tx_url = exp.tx_url(&tx_ref);
                    eprintln!("Channel settled: {}", tx_url);
                } else {
                    eprintln!("Channel settled: {}", tx_ref);
                }
            }
        }
    } else {
        eprintln!("Channel close sent (no receipt)");
    }

    Ok(())
}

/// Submit requestClose() or withdraw() directly on-chain as a Tempo type-0x76 transaction.
///
/// The escrow contract's payer-initiated close is a two-step process:
/// 1. `requestClose(channelId)` — starts a 15-minute grace period
/// 2. `withdraw(channelId)` — after the grace period, refunds deposit minus settled
///
/// This path works regardless of the authorized signer, since only the payer
/// wallet is required. No voucher signature is needed.
///
/// This function checks the channel's `closeRequestedAt` timestamp:
/// - If 0: submits `requestClose()` and returns `Pending`
/// - If non-zero and grace period elapsed: submits `withdraw()` and returns `Closed`
/// - If non-zero but grace period not elapsed: returns `Pending`
async fn close_on_chain(
    config: &Config,
    record: &session_store::SessionRecord,
    wallet: &crate::wallet::signer::WalletSigner,
    channel_id: B256,
    escrow_contract: Address,
) -> Result<CloseOutcome> {
    use alloy::primitives::{Bytes, TxKind, U256};
    use alloy::sol;
    use alloy::sol_types::SolCall;
    use tempo_primitives::transaction::Call;

    sol! {
        interface IEscrow {
            function requestClose(bytes32 channelId) external;
            function withdraw(bytes32 channelId) external;
        }
    }

    let network = Network::from_chain_id(record.chain_id).ok_or_else(|| {
        crate::error::PrestoError::InvalidConfig(format!(
            "Unsupported chainId: {}",
            record.chain_id
        ))
    })?;
    let network_info = config.resolve_network(network.as_str())?;
    let rpc_url: url::Url = network_info
        .rpc_url
        .parse()
        .map_err(|e| crate::error::PrestoError::InvalidConfig(format!("invalid RPC URL: {}", e)))?;
    let provider = alloy::providers::RootProvider::new_http(rpc_url);

    // Check current channel state to determine which step we're on
    let on_chain = get_channel_on_chain(&provider, escrow_contract, channel_id, B256::ZERO)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Channel no longer exists on-chain"))?;

    let fee_token: Address = record
        .currency
        .parse()
        .context("Invalid currency address in session record")?;

    let from = wallet.from;

    // If closeRequestedAt is 0, we need to call requestClose() first
    if on_chain.close_requested_at == 0 {
        let request_close_data = Bytes::from(
            IEscrow::requestCloseCall {
                channelId: channel_id,
            }
            .abi_encode(),
        );

        let calls = vec![Call {
            to: TxKind::Call(escrow_contract),
            value: U256::ZERO,
            input: request_close_data,
        }];

        let tx_hash =
            submit_tempo_tx(&provider, wallet, record.chain_id, fee_token, from, calls).await?;

        let explorer = network.info().explorer;
        let tx_url = explorer
            .as_ref()
            .map(|exp| exp.tx_url(&tx_hash))
            .unwrap_or(tx_hash);
        tracing::info!("requestClose TX: {}", tx_url);

        let grace_secs = read_grace_period(&provider, escrow_contract)
            .await
            .unwrap_or(900);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let ready_at = now + grace_secs;

        if let Err(e) =
            session_store::save_pending_close(&record.channel_id, &record.network_name, ready_at)
        {
            tracing::warn!(%e, "failed to persist pending close for automatic finalization");
        }

        return Ok(CloseOutcome::Pending {
            remaining_secs: grace_secs,
        });
    }

    // closeRequestedAt is non-zero — check if grace period has elapsed
    let grace_period = read_grace_period(&provider, escrow_contract)
        .await
        .unwrap_or(900);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let ready_at = on_chain.close_requested_at as u64 + grace_period;
    if now < ready_at {
        let remaining = ready_at - now;
        return Ok(CloseOutcome::Pending {
            remaining_secs: remaining,
        });
    }

    // Grace period elapsed — submit withdraw() to reclaim deposit
    let withdraw_data = Bytes::from(
        IEscrow::withdrawCall {
            channelId: channel_id,
        }
        .abi_encode(),
    );

    let calls = vec![Call {
        to: TxKind::Call(escrow_contract),
        value: U256::ZERO,
        input: withdraw_data,
    }];

    let tx_hash =
        submit_tempo_tx(&provider, wallet, record.chain_id, fee_token, from, calls).await?;

    let explorer = network.info().explorer;
    let tx_url = explorer
        .as_ref()
        .map(|exp| exp.tx_url(&tx_hash))
        .unwrap_or(tx_hash);
    tracing::info!("withdraw TX: {}", tx_url);

    Ok(CloseOutcome::Closed)
}

/// Read CLOSE_GRACE_PERIOD from the escrow contract. Returns None on error.
async fn read_grace_period(
    provider: &alloy::providers::RootProvider<alloy::network::Ethereum>,
    escrow_contract: Address,
) -> Option<u64> {
    use alloy::providers::Provider;
    use alloy::sol;
    use alloy::sol_types::SolCall;

    sol! {
        interface IEscrow {
            function CLOSE_GRACE_PERIOD() external view returns (uint64 period);
        }
    }

    let call_data = IEscrow::CLOSE_GRACE_PERIODCall {}.abi_encode();
    let tx = alloy::rpc::types::TransactionRequest::default()
        .to(escrow_contract)
        .input(alloy::primitives::Bytes::from(call_data).into());

    let result = provider.call(tx).await.ok()?;
    let decoded: u64 = IEscrow::CLOSE_GRACE_PERIODCall::abi_decode_returns(&result).ok()?;
    Some(decoded)
}

/// Submit a Tempo type-0x76 transaction and return the tx hash.
async fn submit_tempo_tx(
    provider: &alloy::providers::RootProvider<alloy::network::Ethereum>,
    wallet: &crate::wallet::signer::WalletSigner,
    chain_id: u64,
    fee_token: Address,
    from: Address,
    calls: Vec<tempo_primitives::transaction::Call>,
) -> Result<String> {
    use alloy::primitives::U256;
    use alloy::providers::Provider;

    let resolved = mpp::client::tempo::gas::resolve_gas_with_stuck_detection(
        provider,
        from,
        1_000_000_000,
        1_000_000_000,
    )
    .await
    .map_err(|e| crate::error::PrestoError::Http(e.to_string()))?;

    let gas_limit = mpp::client::tempo::tx_builder::estimate_gas(
        provider,
        from,
        chain_id,
        resolved.nonce,
        fee_token,
        &calls,
        resolved.max_fee_per_gas,
        resolved.max_priority_fee_per_gas,
        wallet.signing_mode.key_authorization(),
    )
    .await
    .map_err(|e| crate::error::PrestoError::SigningSimple(e.to_string()))?;

    let tx = mpp::client::tempo::tx_builder::build_tempo_tx(
        mpp::client::tempo::tx_builder::TempoTxOptions {
            calls,
            chain_id,
            fee_token,
            nonce: resolved.nonce,
            nonce_key: U256::ZERO,
            gas_limit,
            max_fee_per_gas: resolved.max_fee_per_gas,
            max_priority_fee_per_gas: resolved.max_priority_fee_per_gas,
            fee_payer: false,
            valid_before: None,
            key_authorization: wallet.signing_mode.key_authorization().cloned(),
        },
    );

    let tx_bytes = mpp::client::tempo::signing::sign_and_encode_async(
        tx,
        &wallet.signer,
        &wallet.signing_mode,
    )
    .await
    .map_err(|e| crate::error::PrestoError::SigningSimple(e.to_string()))?;

    let pending = provider
        .send_raw_transaction(&tx_bytes)
        .await
        .context("Failed to broadcast transaction")?;

    Ok(format!("{:#x}", pending.tx_hash()))
}

/// Close a discovered on-chain channel directly, without a server.
///
/// Uses the payer-initiated path (`requestClose` → `withdraw`) which works
/// regardless of whether the current access key matches the channel's
/// `authorizedSigner`. This allows closing orphaned channels after key
/// rotation or expiry.
pub async fn close_discovered_channel(
    channel: &DiscoveredChannel,
    config: &Config,
) -> Result<CloseOutcome> {
    let network: Network = channel
        .network
        .parse()
        .map_err(|_| anyhow::anyhow!("Unknown network: {}", channel.network))?;

    let wallet = load_wallet_signer(network.as_str())?;

    let channel_id: B256 = channel.channel_id.parse().context("Invalid channel_id")?;
    let escrow_contract: Address = channel
        .escrow_contract
        .parse()
        .context("Invalid escrow_contract")?;
    let chain_id = network.chain_id();

    let record_stub = session_store::SessionRecord {
        version: 1,
        origin: String::new(),
        request_url: String::new(),
        network_name: channel.network.clone(),
        chain_id,
        channel_id: channel.channel_id.clone(),
        escrow_contract: channel.escrow_contract.clone(),
        currency: channel.token.clone(),
        cumulative_amount: "0".to_string(),
        deposit: String::new(),
        did: String::new(),
        challenge_echo: String::new(),
        challenge_id: String::new(),
        salt: String::new(),
        recipient: channel.payee.clone(),
        payer: String::new(),
        authorized_signer: channel.authorized_signer.clone(),
        tick_cost: String::new(),
        created_at: 0,
        last_used_at: 0,
        expires_at: 0,
    };

    close_on_chain(config, &record_stub, &wallet, channel_id, escrow_contract).await
}

/// Close a channel by its on-chain ID, scanning all networks to find it.
///
/// Uses the payer-initiated path (`requestClose` → `withdraw`) which works
/// regardless of the channel's authorized signer. This allows closing
/// orphaned channels after access key rotation or expiry.
pub async fn close_channel_by_id(
    config: &Config,
    channel_id_hex: &str,
    network_filter: Option<&str>,
) -> Result<CloseOutcome> {
    let channel_id: B256 = channel_id_hex
        .parse()
        .context("Invalid channel ID (expected 0x-prefixed bytes32 hex)")?;

    let wallet = load_wallet_signer(
        network_filter.unwrap_or(
            Network::all()
                .first()
                .map(|n| n.as_str())
                .unwrap_or("tempo"),
        ),
    )?;

    let networks: Vec<Network> = if let Some(name) = network_filter {
        name.parse::<Network>().ok().into_iter().collect()
    } else {
        Network::all().to_vec()
    };

    for network in &networks {
        let network_info = match config.resolve_network(network.as_str()) {
            Ok(info) => info,
            Err(_) => continue,
        };
        let rpc_url: url::Url = match network_info.rpc_url.parse() {
            Ok(u) => u,
            Err(_) => continue,
        };
        let provider =
            alloy::providers::RootProvider::<alloy::network::Ethereum>::new_http(rpc_url);

        let escrow: Address = match network.escrow_contract().parse() {
            Ok(a) => a,
            Err(_) => continue,
        };

        let on_chain = match get_channel_on_chain(&provider, escrow, channel_id, B256::ZERO).await {
            Ok(Some(ch)) => ch,
            Ok(None) => continue,
            Err(e) => {
                tracing::debug!(network = network.as_str(), %e, "failed to query channel");
                continue;
            }
        };

        let token = on_chain.token;
        let chain_id = network.chain_id();

        let record_stub = session_store::SessionRecord {
            version: 1,
            origin: String::new(),
            request_url: String::new(),
            network_name: network.as_str().to_string(),
            chain_id,
            channel_id: channel_id_hex.to_string(),
            escrow_contract: format!("{:#x}", escrow),
            currency: format!("{:#x}", token),
            cumulative_amount: "0".to_string(),
            deposit: String::new(),
            did: String::new(),
            challenge_echo: String::new(),
            challenge_id: String::new(),
            salt: String::new(),
            recipient: String::new(),
            payer: String::new(),
            authorized_signer: String::new(),
            tick_cost: String::new(),
            created_at: 0,
            last_used_at: 0,
            expires_at: 0,
        };

        return close_on_chain(config, &record_stub, &wallet, channel_id, escrow).await;
    }

    anyhow::bail!("Channel {} not found on any network", channel_id_hex)
}

/// Query on-chain state for a channel by its hex ID and network name.
///
/// Returns `Some((token_address, deposit, settled, network_name))` if the channel
/// exists on-chain, or `None` if not found or on RPC error.
pub async fn query_channel_state(
    config: &Config,
    channel_id_hex: &str,
    network_name: &str,
) -> Option<(String, u128, u128, String)> {
    let channel_id: B256 = channel_id_hex.parse().ok()?;
    let network: Network = network_name.parse().ok()?;
    let network_info = config.resolve_network(network.as_str()).ok()?;
    let rpc_url: url::Url = network_info.rpc_url.parse().ok()?;
    let provider = alloy::providers::RootProvider::<alloy::network::Ethereum>::new_http(rpc_url);
    let escrow: Address = network.escrow_contract().parse().ok()?;

    let on_chain = get_channel_on_chain(&provider, escrow, channel_id, B256::ZERO)
        .await
        .ok()??;

    Some((
        format!("{:#x}", on_chain.token),
        on_chain.deposit,
        on_chain.settled,
        network_name.to_string(),
    ))
}

/// Derive the network from a session request's chain ID.
fn network_from_session_request(req: &mpp::SessionRequest) -> crate::error::Result<Network> {
    use mpp::protocol::methods::tempo::session::TempoSessionExt;
    let chain_id = req.chain_id().ok_or_else(|| {
        crate::error::PrestoError::InvalidConfig("Missing chainId in session request".to_string())
    })?;
    Network::from_chain_id(chain_id).ok_or_else(|| {
        crate::error::PrestoError::InvalidConfig(format!("Unsupported chainId: {}", chain_id))
    })
}

/// Create a Tempo payment credential from pre-built calls.
///
/// Used by session payments where the calls (e.g., approve + escrow.open)
/// are built externally. Resolves nonce/gas at signing time inside mpp-rs
/// (including stuck-tx detection) and signs with keychain-aware signing mode.
async fn create_tempo_payment_from_calls(
    config: &Config,
    signing: &crate::wallet::signer::WalletSigner,
    challenge: &mpp::PaymentChallenge,
    calls: Vec<tempo_primitives::transaction::Call>,
    fee_token: Address,
    chain_id: u64,
) -> Result<mpp::PaymentCredential> {
    let network = Network::from_chain_id(chain_id).ok_or_else(|| {
        crate::error::PrestoError::InvalidConfig(format!("Unsupported chainId: {}", chain_id))
    })?;
    let network_info = config.resolve_network(network.as_str())?;

    let rpc_url: url::Url = network_info
        .rpc_url
        .parse()
        .map_err(|e| crate::error::PrestoError::InvalidConfig(format!("invalid RPC URL: {}", e)))?;
    let provider = alloy::providers::RootProvider::new_http(rpc_url);

    let from = signing.from;

    // Resolve nonce and gas with stuck-tx detection
    let resolved = mpp::client::tempo::gas::resolve_gas_with_stuck_detection(
        &provider,
        from,
        1_000_000_000, // 1 gwei default max fee
        1_000_000_000, // 1 gwei default priority fee
    )
    .await
    .map_err(|e| crate::error::PrestoError::Http(e.to_string()))?;

    // Estimate gas
    let gas_limit = mpp::client::tempo::tx_builder::estimate_gas(
        &provider,
        from,
        chain_id,
        resolved.nonce,
        fee_token,
        &calls,
        resolved.max_fee_per_gas,
        resolved.max_priority_fee_per_gas,
        signing.signing_mode.key_authorization(),
    )
    .await
    .map_err(|e| crate::error::PrestoError::SigningSimple(e.to_string()))?;

    // Build and sign the transaction
    let tx = mpp::client::tempo::tx_builder::build_tempo_tx(
        mpp::client::tempo::tx_builder::TempoTxOptions {
            calls,
            chain_id,
            fee_token,
            nonce: resolved.nonce,
            nonce_key: alloy::primitives::U256::ZERO,
            gas_limit,
            max_fee_per_gas: resolved.max_fee_per_gas,
            max_priority_fee_per_gas: resolved.max_priority_fee_per_gas,
            fee_payer: false,
            valid_before: None,
            key_authorization: signing.signing_mode.key_authorization().cloned(),
        },
    );

    let tx_bytes = mpp::client::tempo::signing::sign_and_encode_async(
        tx,
        &signing.signer,
        &signing.signing_mode,
    )
    .await
    .map_err(|e| crate::error::PrestoError::SigningSimple(e.to_string()))?;

    Ok(mpp::client::tempo::tx_builder::build_charge_credential(
        challenge, &tx_bytes, chain_id, from,
    ))
}
