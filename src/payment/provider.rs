//! Payment provider abstraction for tempoctl.
//!
//! This module provides payment providers that implement the mpay::client::PaymentProvider trait,
//! enabling automatic Web Payment Auth handling with tempoctl-specific features like keychain signing.

use crate::config::Config;
use crate::error::{Result, TempoCtlError};
use crate::network::Network;
use crate::payment::money::format_u256_with_decimals;
use crate::payment::providers::tempo::{
    pending_key_spending_limit, query_key_spending_limit, SwapInfo, BPS_DENOMINATOR,
    SWAP_SLIPPAGE_BPS,
};
use crate::wallet::signer::load_signer_with_priority;
use alloy::primitives::{Address, U256};
use alloy::providers::ProviderBuilder;
use alloy::rlp::Decodable;
use alloy::sol;
use std::str::FromStr;
use std::sync::Arc;
use tempo_primitives::transaction::SignedKeyAuthorization;
use tracing::{debug, info};

/// Balance information for a single token on a network
#[derive(Debug, Clone)]
pub struct NetworkBalance {
    /// The network this balance is for (typed enum)
    pub network: Network,
    /// The balance as a typed U256 value
    pub balance: U256,
    /// Human-readable balance string (for display)
    pub balance_human: String,
    /// Asset symbol (e.g., "pathUSD", "AlphaUSD")
    pub asset: String,
}

impl NetworkBalance {
    /// Create a new NetworkBalance.
    pub fn new(network: Network, balance: U256, balance_human: String, asset: String) -> Self {
        Self {
            network,
            balance,
            balance_human,
            asset,
        }
    }
}

impl std::fmt::Display for NetworkBalance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {} on {}",
            self.balance_human, self.asset, self.network
        )
    }
}

/// TempoCtl payment provider that wraps config and implements mpay::client::PaymentProvider.
///
/// This provider handles both Tempo and EVM networks, automatically selecting
/// the appropriate transaction format based on the payment method.
///
/// When `no_swap` is false (default), the provider will automatically swap from
/// a different stablecoin if the user doesn't have the required token.
#[derive(Clone)]
pub struct TempoCtlPaymentProvider {
    config: Arc<Config>,
    /// If true, disable automatic token swaps.
    no_swap: bool,
    /// If true, force close the stream channel on next request.
    close_stream: bool,
}

impl TempoCtlPaymentProvider {
    /// Create a new provider with the given configuration.
    #[allow(dead_code)]
    pub fn new(config: Config) -> Self {
        Self {
            config: Arc::new(config),
            no_swap: false,
            close_stream: false,
        }
    }

    /// Create a new provider with swap behavior configured.
    #[allow(dead_code)]
    pub fn with_no_swap(config: Config, no_swap: bool) -> Self {
        Self {
            config: Arc::new(config),
            no_swap,
            close_stream: false,
        }
    }

    /// Create a new provider with all options configured.
    pub fn with_options(config: Config, no_swap: bool, close_stream: bool) -> Self {
        Self {
            config: Arc::new(config),
            no_swap,
            close_stream,
        }
    }
}

impl mpay::client::PaymentProvider for TempoCtlPaymentProvider {
    fn supports(&self, method: &str, intent: &str) -> bool {
        let method_lower = method.to_lowercase();
        let is_supported_method = method_lower == "tempo";
        let is_supported_intent = intent == "charge" || intent == "stream";
        is_supported_method && is_supported_intent
    }

    async fn pay(
        &self,
        challenge: &mpay::PaymentChallenge,
    ) -> std::result::Result<mpay::PaymentCredential, mpay::MppError> {
        let method = challenge.method.as_str().to_lowercase();

        match method.as_str() {
            "tempo" if challenge.intent.is_stream() => {
                self.create_tempo_stream_payment(challenge).await
            }
            "tempo" => self.create_tempo_payment(challenge).await,
            _ => Err(mpay::MppError::UnsupportedPaymentMethod(format!(
                "Payment method '{}' is not supported",
                challenge.method
            ))),
        }
    }
}

impl TempoCtlPaymentProvider {
    async fn create_tempo_payment(
        &self,
        challenge: &mpay::PaymentChallenge,
    ) -> std::result::Result<mpay::PaymentCredential, mpay::MppError> {
        use crate::payment::mpay_ext::{method_to_network, TempoChargeExt};
        use crate::payment::providers::tempo::{
            create_tempo_payment, create_tempo_payment_with_swap,
        };

        let charge_req: mpay::ChargeRequest = challenge
            .request
            .decode()
            .map_err(|e| mpay::MppError::Http(format!("Invalid charge request: {}", e)))?;

        let required_token = charge_req
            .currency_address()
            .map_err(|e| mpay::MppError::Http(format!("Invalid currency address: {}", e)))?;
        let required_amount = charge_req
            .amount_u256()
            .map_err(|e| mpay::MppError::Http(format!("Invalid amount: {}", e)))?;

        let signer_ctx =
            load_signer_with_priority().map_err(|e| mpay::MppError::Http(e.to_string()))?;

        let key_address = signer_ctx.signer.address();
        let wallet_address = signer_ctx
            .wallet_address
            .as_ref()
            .map(|addr| Address::from_str(addr))
            .transpose()
            .map_err(|e| mpay::MppError::Http(format!("Invalid wallet address: {}", e)))?;

        let pending_auth = signer_ctx
            .pending_key_authorization
            .as_ref()
            .map(|hex_str| {
                let hex_str = hex_str.strip_prefix("0x").unwrap_or(hex_str);
                let bytes = hex::decode(hex_str).map_err(|e| {
                    mpay::MppError::Http(format!("Invalid pending key authorization hex: {}", e))
                })?;
                let mut slice = bytes.as_slice();
                SignedKeyAuthorization::decode(&mut slice).map_err(|e| {
                    mpay::MppError::Http(format!("Invalid pending key authorization RLP: {}", e))
                })
            })
            .transpose()?;

        let from = wallet_address.unwrap_or(key_address);

        let network_name = method_to_network(&challenge.method).ok_or_else(|| {
            mpay::MppError::UnsupportedPaymentMethod(format!(
                "Unsupported payment method: {}",
                challenge.method
            ))
        })?;
        let network = Network::from_str(network_name)
            .map_err(|e| mpay::MppError::Http(format!("Unknown network: {}", e)))?;

        let token_config = network.token_config_by_address(&format!("{:#x}", required_token));
        let token_symbol = token_config
            .map(|t| t.currency.symbol.to_string())
            .unwrap_or_else(|| format!("{:#x}", required_token));
        let token_decimals = token_config.map(|t| t.currency.decimals).unwrap_or(6);

        debug!(
            %from,
            network = %network,
            required_token = %token_symbol,
            required_token_address = %format!("{:#x}", required_token),
            required_amount = %format_u256_with_decimals(required_amount, token_decimals),
            signing_mode = if wallet_address.is_some() { "keychain" } else { "direct" },
            "resolving payment"
        );

        let balance = query_token_balance(&self.config, network, required_token, from)
            .await
            .map_err(|e| mpay::MppError::Http(e.to_string()))?;

        debug!(
            balance = %format_u256_with_decimals(balance, token_decimals),
            token = %token_symbol,
            "queried wallet balance"
        );

        let spending_limit = if let Some(wallet_addr) = wallet_address {
            let network_info = self
                .config
                .resolve_network(network_name)
                .map_err(|e| mpay::MppError::Http(e.to_string()))?;
            let provider = ProviderBuilder::new().connect_http(
                network_info
                    .rpc_url
                    .parse()
                    .map_err(|e| mpay::MppError::Http(format!("Invalid RPC URL: {}", e)))?,
            );

            let limit =
                match query_key_spending_limit(&provider, wallet_addr, key_address, required_token)
                    .await
                {
                    Ok(limit) => limit,
                    Err(_) if pending_auth.is_some() => {
                        pending_key_spending_limit(pending_auth.as_ref().unwrap(), required_token)
                    }
                    Err(e) => {
                        return Err(mpay::MppError::Http(format!(
                            "Cannot verify key spending limit for {}: {}. \
                         Refusing to proceed — the key may not be authorized for this token.",
                            token_symbol, e
                        )));
                    }
                };

            if let Some(remaining) = limit {
                if remaining < balance {
                    eprintln!(
                        "Key spending limit ({} {}) is lower than wallet balance",
                        format_u256_with_decimals(remaining, token_decimals),
                        token_symbol,
                    );
                }
            }

            limit
        } else {
            None
        };

        let effective_capacity = effective_capacity(balance, spending_limit);

        if effective_capacity >= required_amount {
            debug!(
                tx_type = "direct",
                token = %token_symbol,
                amount = %format_u256_with_decimals(required_amount, token_decimals),
                gas_token = %token_symbol,
                "building direct transfer"
            );
            return create_tempo_payment(&self.config, challenge)
                .await
                .map_err(|e| mpay::MppError::Http(e.to_string()));
        }

        let limit_is_bottleneck = spending_limit
            .map(|limit| limit < required_amount)
            .unwrap_or(false);

        if limit_is_bottleneck {
            let limit_human =
                format_u256_with_decimals(spending_limit.unwrap_or(U256::ZERO), token_decimals);
            let needed_human = format_u256_with_decimals(required_amount, token_decimals);
            return Err(mpay::MppError::Http(format!(
                "Key spending limit too low: limit is {} {} but payment requires {} {}. \
                 A swap cannot help because the key must also be authorized to transfer the \
                 destination token. Increase the key's spending limit for {} or use a key \
                 without enforced limits.",
                limit_human, token_symbol, needed_human, token_symbol, token_symbol
            )));
        }

        if self.no_swap {
            return Err(mpay::MppError::Http(format!(
                "Insufficient {} balance: have {}, need {}. Use a different token or remove --no-swap to enable automatic swaps.",
                token_symbol,
                format_u256_with_decimals(balance, token_decimals),
                format_u256_with_decimals(required_amount, token_decimals),
            )));
        }

        let keychain_info = wallet_address.map(|wa| (wa, key_address));
        let swap_source = find_swap_source(
            &self.config,
            network,
            from,
            required_token,
            required_amount,
            keychain_info,
            pending_auth.as_ref(),
        )
        .await
        .map_err(|e| mpay::MppError::Http(e.to_string()))?;

        match swap_source {
            Some(source) => {
                eprintln!(
                    "Auto-swapping from {} to {} to complete payment",
                    source.symbol, token_symbol
                );

                let swap_info =
                    SwapInfo::new(source.token_address, required_token, required_amount);

                debug!(
                    tx_type = "swap",
                    token_in = %source.symbol,
                    token_in_address = %format!("{:#x}", source.token_address),
                    token_out = %token_symbol,
                    token_out_address = %format!("{:#x}", required_token),
                    amount_out = %format_u256_with_decimals(required_amount, token_decimals),
                    max_amount_in = %format_u256_with_decimals(swap_info.max_amount_in, token_decimals),
                    slippage_bps = SWAP_SLIPPAGE_BPS,
                    gas_token = %source.symbol,
                    "building swap transaction (approve → swap → transfer)"
                );

                create_tempo_payment_with_swap(&self.config, challenge, &swap_info)
                    .await
                    .map_err(|e| mpay::MppError::Http(e.to_string()))
            }
            None => Err(mpay::MppError::Http(format!(
                "Insufficient balance and no swap source available. Need {} of {}",
                format_u256_with_decimals(required_amount, token_decimals),
                token_symbol
            ))),
        }
    }

    async fn create_tempo_stream_payment(
        &self,
        challenge: &mpay::PaymentChallenge,
    ) -> std::result::Result<mpay::PaymentCredential, mpay::MppError> {
        use crate::payment::mpay_ext::{method_to_network, TempoStreamExt};
        use crate::payment::providers::stream::{
            compute_channel_id, query_on_chain_channel, resolve_chain_id, resolve_deposit,
            resolve_escrow_contract, sign_voucher, ChannelEntry, StreamState,
        };
        use crate::payment::providers::tempo::{
            create_tempo_stream_close, create_tempo_stream_open, create_tempo_stream_top_up,
        };
        use alloy::primitives::B256;

        let stream_req: mpay::StreamRequest = challenge
            .request
            .decode()
            .map_err(|e| mpay::MppError::Http(format!("Invalid stream request: {}", e)))?;

        let required_token = stream_req
            .currency_address()
            .map_err(|e| mpay::MppError::Http(format!("Invalid currency address: {}", e)))?;
        let per_unit_amount: u128 = stream_req
            .amount
            .parse()
            .map_err(|e| mpay::MppError::Http(format!("Invalid amount: {}", e)))?;

        let signer_ctx =
            load_signer_with_priority().map_err(|e| mpay::MppError::Http(e.to_string()))?;
        let signer = signer_ctx.signer;
        let wallet_address = signer_ctx
            .wallet_address
            .as_ref()
            .map(|addr| Address::from_str(addr))
            .transpose()
            .map_err(|e| mpay::MppError::Http(format!("Invalid wallet address: {}", e)))?;
        let from = wallet_address.unwrap_or_else(|| signer.address());

        let network_name = method_to_network(&challenge.method).ok_or_else(|| {
            mpay::MppError::UnsupportedPaymentMethod(format!(
                "Unsupported payment method: {}",
                challenge.method
            ))
        })?;
        let _network = Network::from_str(network_name)
            .map_err(|e| mpay::MppError::Http(format!("Unknown network: {}", e)))?;
        let network_info = self
            .config
            .resolve_network(network_name)
            .map_err(|e| mpay::MppError::Http(e.to_string()))?;
        let fallback_chain_id = network_info.chain_id.unwrap_or(42431);

        let escrow_contract = resolve_escrow_contract(stream_req.method_details.as_ref());
        let chain_id = resolve_chain_id(stream_req.method_details.as_ref(), fallback_chain_id);
        let payee = stream_req
            .recipient
            .as_ref()
            .and_then(|r| r.parse::<Address>().ok())
            .ok_or_else(|| mpay::MppError::Http("Stream request missing recipient".to_string()))?;

        let mut state = StreamState::load()
            .map_err(|e| mpay::MppError::Http(format!("Failed to load stream state: {}", e)))?;
        let key =
            StreamState::channel_key(&from, &payee, &required_token, &escrow_contract, chain_id);

        let did = format!("did:pkh:eip155:{}:{:#x}", chain_id, from);

        let explicit_action = stream_req
            .method_details
            .as_ref()
            .and_then(|md| md.get("action"))
            .and_then(|v| v.as_str());

        if explicit_action == Some("close") || self.close_stream {
            if let Some(entry) = state.channels.remove(&key) {
                let channel_id_bytes: B256 = entry.channel_id.parse().map_err(|e| {
                    mpay::MppError::Http(format!("Invalid stored channel ID: {}", e))
                })?;
                let cumulative: u128 = entry.cumulative_amount.parse().unwrap_or(0);
                let token: Address = entry
                    .token
                    .parse()
                    .map_err(|e| mpay::MppError::Http(format!("Invalid stored token: {}", e)))?;

                let voucher_sig = sign_voucher(
                    &signer,
                    channel_id_bytes,
                    cumulative,
                    escrow_contract,
                    chain_id,
                )
                .map_err(|e| mpay::MppError::Http(e.to_string()))?;

                let sig_hex = voucher_sig.strip_prefix("0x").unwrap_or(&voucher_sig);
                let sig_bytes = hex::decode(sig_hex)
                    .map_err(|e| mpay::MppError::Http(format!("Invalid voucher sig hex: {}", e)))?;

                let signed_tx = create_tempo_stream_close(
                    &self.config,
                    challenge,
                    token,
                    escrow_contract,
                    channel_id_bytes,
                    cumulative,
                    sig_bytes,
                )
                .await
                .map_err(|e| mpay::MppError::Http(e.to_string()))?;

                state.save().map_err(|e| {
                    mpay::MppError::Http(format!("Failed to save stream state: {}", e))
                })?;

                info!(
                    channel_id = %entry.channel_id,
                    cumulative_amount = cumulative,
                    "closing stream channel"
                );

                let payload = serde_json::json!({
                    "action": "close",
                    "type": "transaction",
                    "channelId": entry.channel_id,
                    "transaction": format!("0x{}", signed_tx),
                    "cumulativeAmount": cumulative.to_string(),
                    "signature": voucher_sig,
                });

                return Ok(mpay::PaymentCredential::with_raw_payload(
                    challenge.to_echo(),
                    &did,
                    payload,
                ));
            } else {
                return Err(mpay::MppError::Http(
                    "No existing channel found for close action".to_string(),
                ));
            }
        }

        if let Some(entry) = state.channels.get(&key) {
            let prev: u128 = entry.cumulative_amount.parse().unwrap_or(0);
            let new_cumulative = prev + per_unit_amount;
            let current_deposit: u128 = entry.deposit.parse().unwrap_or(0);
            let channel_id_str = entry.channel_id.clone();

            let channel_id_bytes: B256 = channel_id_str
                .parse()
                .map_err(|e| mpay::MppError::Http(format!("Invalid stored channel ID: {}", e)))?;

            if new_cumulative > current_deposit {
                let additional = new_cumulative - current_deposit
                    + resolve_deposit(stream_req.suggested_deposit.as_deref());

                let signed_tx = create_tempo_stream_top_up(
                    &self.config,
                    challenge,
                    required_token,
                    escrow_contract,
                    channel_id_bytes,
                    additional,
                )
                .await
                .map_err(|e| mpay::MppError::Http(e.to_string()))?;

                let voucher_sig = sign_voucher(
                    &signer,
                    channel_id_bytes,
                    new_cumulative,
                    escrow_contract,
                    chain_id,
                )
                .map_err(|e| mpay::MppError::Http(e.to_string()))?;

                let entry = state.channels.get_mut(&key).unwrap();
                entry.deposit = (current_deposit + additional).to_string();
                entry.cumulative_amount = new_cumulative.to_string();

                state.save().map_err(|e| {
                    mpay::MppError::Http(format!("Failed to save stream state: {}", e))
                })?;

                info!(
                    channel_id = %channel_id_str,
                    additional_deposit = additional,
                    cumulative_amount = new_cumulative,
                    "topping up stream channel"
                );

                let payload = serde_json::json!({
                    "action": "topUp",
                    "type": "transaction",
                    "channelId": channel_id_str,
                    "transaction": format!("0x{}", signed_tx),
                    "additionalDeposit": additional.to_string(),
                    "cumulativeAmount": new_cumulative.to_string(),
                    "signature": voucher_sig,
                });

                return Ok(mpay::PaymentCredential::with_raw_payload(
                    challenge.to_echo(),
                    &did,
                    payload,
                ));
            }

            let entry = state.channels.get_mut(&key).unwrap();
            entry.cumulative_amount = new_cumulative.to_string();
            let _ = entry;

            let voucher_sig = sign_voucher(
                &signer,
                channel_id_bytes,
                new_cumulative,
                escrow_contract,
                chain_id,
            )
            .map_err(|e| mpay::MppError::Http(e.to_string()))?;

            state
                .save()
                .map_err(|e| mpay::MppError::Http(format!("Failed to save stream state: {}", e)))?;

            info!(
                channel_id = %channel_id_str,
                cumulative_amount = new_cumulative,
                "issuing stream voucher"
            );

            let payload = serde_json::json!({
                "action": "voucher",
                "channelId": channel_id_str,
                "cumulativeAmount": new_cumulative.to_string(),
                "signature": voucher_sig,
            });

            Ok(mpay::PaymentCredential::with_raw_payload(
                challenge.to_echo(),
                &did,
                payload,
            ))
        } else {
            let hint_channel_id = stream_req
                .method_details
                .as_ref()
                .and_then(|md| md.get("channelId"))
                .and_then(|v| v.as_str());

            if let Some(channel_id_hex) = hint_channel_id {
                let channel_id_bytes: B256 = channel_id_hex.parse().map_err(|e| {
                    mpay::MppError::Http(format!("Invalid channelId in method_details: {}", e))
                })?;

                let rpc_url: reqwest::Url = network_info
                    .rpc_url
                    .parse()
                    .map_err(|e| mpay::MppError::Http(format!("Invalid RPC URL: {}", e)))?;
                let provider = ProviderBuilder::new().connect_http(rpc_url);

                if let Ok(Some(on_chain)) =
                    query_on_chain_channel(&provider, escrow_contract, channel_id_bytes).await
                {
                    if on_chain.authorized_signer == signer.address() && !on_chain.finalized {
                        let new_cumulative = on_chain.settled + per_unit_amount;

                        let entry = ChannelEntry {
                            channel_id: format!("{:#x}", channel_id_bytes),
                            salt:
                                "0x0000000000000000000000000000000000000000000000000000000000000000"
                                    .to_string(),
                            deposit: on_chain.deposit.to_string(),
                            cumulative_amount: new_cumulative.to_string(),
                            payer: format!("{:#x}", on_chain.payer),
                            payee: format!("{:#x}", on_chain.payee),
                            token: format!("{:#x}", on_chain.token),
                            escrow_contract: format!("{:#x}", escrow_contract),
                            chain_id,
                            authorized_signer: format!("{:#x}", on_chain.authorized_signer),
                            close_requested_at: 0,
                        };
                        state.channels.insert(key, entry);

                        let voucher_sig = sign_voucher(
                            &signer,
                            channel_id_bytes,
                            new_cumulative,
                            escrow_contract,
                            chain_id,
                        )
                        .map_err(|e| mpay::MppError::Http(e.to_string()))?;

                        state.save().map_err(|e| {
                            mpay::MppError::Http(format!("Failed to save stream state: {}", e))
                        })?;

                        info!(
                            channel_id = %channel_id_hex,
                            cumulative_amount = new_cumulative,
                            "recovered channel from on-chain state, issuing voucher"
                        );

                        let payload = serde_json::json!({
                            "action": "voucher",
                            "channelId": channel_id_hex,
                            "cumulativeAmount": new_cumulative.to_string(),
                            "signature": voucher_sig,
                        });

                        return Ok(mpay::PaymentCredential::with_raw_payload(
                            challenge.to_echo(),
                            &did,
                            payload,
                        ));
                    }
                }
            }

            let deposit = resolve_deposit(stream_req.suggested_deposit.as_deref());
            let salt = B256::random();
            let authorized_signer = signer.address();

            let channel_id = compute_channel_id(
                from,
                payee,
                required_token,
                deposit,
                salt,
                authorized_signer,
                escrow_contract,
                chain_id,
            );

            let signed_tx = create_tempo_stream_open(
                &self.config,
                challenge,
                required_token,
                escrow_contract,
                payee,
                deposit,
                salt.0,
                authorized_signer,
            )
            .await
            .map_err(|e| mpay::MppError::Http(e.to_string()))?;

            let initial_amount = per_unit_amount;
            let voucher_sig = sign_voucher(
                &signer,
                channel_id,
                initial_amount,
                escrow_contract,
                chain_id,
            )
            .map_err(|e| mpay::MppError::Http(e.to_string()))?;

            let entry = ChannelEntry {
                channel_id: format!("{:#x}", channel_id),
                salt: format!("{:#x}", salt),
                deposit: deposit.to_string(),
                cumulative_amount: initial_amount.to_string(),
                payer: format!("{:#x}", from),
                payee: format!("{:#x}", payee),
                token: format!("{:#x}", required_token),
                escrow_contract: format!("{:#x}", escrow_contract),
                chain_id,
                authorized_signer: format!("{:#x}", authorized_signer),
                close_requested_at: 0,
            };
            state.channels.insert(key, entry);
            state
                .save()
                .map_err(|e| mpay::MppError::Http(format!("Failed to save stream state: {}", e)))?;

            info!(
                channel_id = %format!("{:#x}", channel_id),
                transaction = %format!("0x{}", &signed_tx[..40.min(signed_tx.len())]),
                deposit,
                initial_amount,
                "opening new stream channel"
            );

            let payload = serde_json::json!({
                "action": "open",
                "type": "transaction",
                "channelId": format!("{:#x}", channel_id),
                "transaction": format!("0x{}", signed_tx),
                "authorizedSigner": format!("{:#x}", authorized_signer),
                "cumulativeAmount": initial_amount.to_string(),
                "signature": voucher_sig,
            });

            Ok(mpay::PaymentCredential::with_raw_payload(
                challenge.to_echo(),
                &did,
                payload,
            ))
        }
    }
}

/// Compute effective spending capacity from wallet balance and optional key spending limit.
///
/// When the key enforces spending limits, the effective capacity is the minimum
/// of the wallet balance and the remaining spending limit. Otherwise, capacity
/// equals the wallet balance.
pub fn effective_capacity(balance: U256, spending_limit: Option<U256>) -> U256 {
    match spending_limit {
        Some(limit) => balance.min(limit),
        None => balance,
    }
}

sol! {
    #[sol(rpc)]
    interface IERC20 {
        function balanceOf(address account) external view returns (uint256);
    }
}

/// Query balances for all supported tokens on a network.
///
/// Returns balances for pathUSD, AlphaUSD, BetaUSD, and ThetaUSD.
pub async fn get_balances(
    config: &Config,
    address: &str,
    network: Network,
) -> Result<Vec<NetworkBalance>> {
    let network_info = config.resolve_network(network.as_str())?;
    let provider =
        ProviderBuilder::new().connect_http(network_info.rpc_url.parse().map_err(|e| {
            TempoCtlError::InvalidConfig(format!("Invalid RPC URL for {network}: {e}"))
        })?);

    let user_addr = Address::from_str(address)
        .map_err(|e| TempoCtlError::invalid_address(format!("Invalid Ethereum address: {e}")))?;

    let mut balances = Vec::new();

    for token_config in network.supported_tokens() {
        let token_addr = Address::from_str(token_config.address).map_err(|e| {
            TempoCtlError::invalid_address(format!(
                "Invalid {} contract address for {}: {}",
                token_config.currency.symbol, network, e
            ))
        })?;

        let contract = IERC20::new(token_addr, &provider);

        match contract.balanceOf(user_addr).call().await {
            Ok(balance) => {
                let balance_human =
                    format_u256_with_decimals(balance, token_config.currency.decimals);
                balances.push(NetworkBalance::new(
                    network,
                    balance,
                    balance_human,
                    token_config.currency.symbol.to_string(),
                ));
            }
            Err(e) => {
                eprintln!(
                    "Warning: Failed to get {} balance on {}: {}",
                    token_config.currency.symbol, network, e
                );
            }
        }
    }

    Ok(balances)
}

/// Query the balance of a specific token for an account.
pub async fn query_token_balance(
    config: &Config,
    network: Network,
    token_address: Address,
    account: Address,
) -> Result<U256> {
    let network_info = config.resolve_network(network.as_str())?;
    let provider =
        ProviderBuilder::new().connect_http(network_info.rpc_url.parse().map_err(|e| {
            TempoCtlError::InvalidConfig(format!("Invalid RPC URL for {network}: {e}"))
        })?);

    let contract = IERC20::new(token_address, &provider);
    let balance = contract
        .balanceOf(account)
        .call()
        .await
        .map_err(|e| TempoCtlError::BalanceQuery(format!("Failed to query balance: {}", e)))?;

    Ok(balance)
}

/// Token with sufficient balance for a swap.
#[derive(Debug, Clone)]
pub struct SwapSource {
    /// Token address that can be used as swap source.
    pub token_address: Address,
    /// Current balance of the token.
    #[allow(dead_code)]
    pub balance: U256,
    /// Human-readable symbol.
    pub symbol: String,
}

/// Find a token with sufficient balance (and spending limit) to swap from.
///
/// When `keychain_info` is provided, queries spending limits first (in parallel),
/// filters and sorts candidates by limit descending, then checks balances only
/// for candidates with sufficient limit. This minimizes on-chain balance queries.
///
/// If the key is not yet provisioned on-chain and `pending_auth` is provided,
/// falls back to the pending authorization's limits to determine token eligibility.
///
/// When no keychain is used, queries all balances in parallel and returns the
/// first token with sufficient balance (including slippage).
///
/// # Arguments
/// * `config` - Configuration for RPC access
/// * `network` - Network to query on
/// * `account` - Account to check balances for
/// * `required_token` - The token the merchant wants (we need to find a different one)
/// * `required_amount` - The amount needed (will include slippage in the check)
/// * `keychain_info` - Optional (wallet_address, key_address) for spending limit checks
/// * `pending_auth` - Optional pending key authorization for local limit validation
///
/// # Returns
/// * `Ok(Some(SwapSource))` - Found a token with sufficient balance and limit
/// * `Ok(None)` - No token qualifies
pub async fn find_swap_source(
    config: &Config,
    network: Network,
    account: Address,
    required_token: Address,
    required_amount: U256,
    keychain_info: Option<(Address, Address)>,
    pending_auth: Option<&SignedKeyAuthorization>,
) -> Result<Option<SwapSource>> {
    use futures::future::join_all;

    let slippage = required_amount * U256::from(SWAP_SLIPPAGE_BPS) / U256::from(BPS_DENOMINATOR);
    let amount_with_slippage = required_amount + slippage;

    let tokens_to_check: Vec<_> = network
        .supported_tokens()
        .into_iter()
        .filter_map(|token_config| {
            let token_address = Address::from_str(token_config.address).ok()?;
            if token_address == required_token {
                None
            } else {
                Some((token_address, token_config.currency.symbol.to_string()))
            }
        })
        .collect();

    if let Some((wallet_addr, key_addr)) = keychain_info {
        let network_info = config.resolve_network(network.as_str())?;
        let provider =
            ProviderBuilder::new().connect_http(network_info.rpc_url.parse().map_err(|e| {
                TempoCtlError::InvalidConfig(format!("Invalid RPC URL for {network}: {e}"))
            })?);

        let limit_futures: Vec<_> = tokens_to_check
            .iter()
            .map(|(token_address, _)| {
                query_key_spending_limit(&provider, wallet_addr, key_addr, *token_address)
            })
            .collect();

        let limits = join_all(limit_futures).await;

        let mut candidates: Vec<_> = tokens_to_check
            .into_iter()
            .zip(limits)
            .filter_map(|((token_address, symbol), limit_result)| {
                let effective = match limit_result {
                    Ok(None) => U256::MAX,
                    Ok(Some(l)) if l >= amount_with_slippage => l,
                    Ok(Some(_)) => return None,
                    Err(_) if pending_auth.is_some() => {
                        match pending_key_spending_limit(pending_auth.unwrap(), token_address) {
                            None => U256::MAX,
                            Some(l) if l >= amount_with_slippage => l,
                            Some(_) => return None,
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "Warning: Failed to query spending limit for {}: {}",
                            symbol, e
                        );
                        return None;
                    }
                };
                Some((token_address, symbol, effective))
            })
            .collect();

        candidates.sort_by(|a, b| b.2.cmp(&a.2));

        for (token_address, symbol, _) in candidates {
            match query_token_balance(config, network, token_address, account).await {
                Ok(balance) if balance >= amount_with_slippage => {
                    return Ok(Some(SwapSource {
                        token_address,
                        balance,
                        symbol,
                    }));
                }
                Ok(_) => {}
                Err(e) => {
                    eprintln!("Warning: Failed to query {} balance: {}", symbol, e);
                }
            }
        }
    } else {
        let balance_futures: Vec<_> = tokens_to_check
            .iter()
            .map(|(token_address, _symbol)| {
                query_token_balance(config, network, *token_address, account)
            })
            .collect();

        let results = join_all(balance_futures).await;

        for ((token_address, symbol), result) in tokens_to_check.into_iter().zip(results) {
            match result {
                Ok(balance) if balance >= amount_with_slippage => {
                    return Ok(Some(SwapSource {
                        token_address,
                        balance,
                        symbol,
                    }));
                }
                Ok(_) => {}
                Err(e) => {
                    eprintln!("Warning: Failed to query {} balance: {}", symbol, e);
                }
            }
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mpay::client::PaymentProvider;

    #[test]
    fn test_provider_supports_tempo() {
        let config = Config::default();
        let provider = TempoCtlPaymentProvider::new(config);

        assert!(provider.supports("tempo", "charge"));
        assert!(provider.supports("tempo", "stream"));
        assert!(provider.supports("TEMPO", "charge"));
        assert!(provider.supports("TEMPO", "stream"));
        assert!(!provider.supports("tempo", "authorize"));
        assert!(!provider.supports("bitcoin", "charge"));
    }

    #[test]
    fn test_provider_rejects_unknown_methods() {
        let config = Config::default();
        let provider = TempoCtlPaymentProvider::new(config);

        assert!(!provider.supports("base", "charge"));
        assert!(!provider.supports("ethereum", "charge"));
        assert!(!provider.supports("bitcoin", "charge"));
        assert!(!provider.supports("unknown", "charge"));
    }

    #[test]
    fn test_effective_capacity_no_spending_limit() {
        let balance = U256::from(1_000_000u64);
        assert_eq!(effective_capacity(balance, None), balance);
    }

    #[test]
    fn test_effective_capacity_limit_below_balance() {
        let balance = U256::from(1_000_000u64);
        let limit = U256::from(500_000u64);
        assert_eq!(effective_capacity(balance, Some(limit)), limit);
    }

    #[test]
    fn test_effective_capacity_limit_above_balance() {
        let balance = U256::from(500_000u64);
        let limit = U256::from(1_000_000u64);
        assert_eq!(effective_capacity(balance, Some(limit)), balance);
    }

    #[test]
    fn test_effective_capacity_limit_equals_balance() {
        let balance = U256::from(1_000_000u64);
        let limit = U256::from(1_000_000u64);
        assert_eq!(effective_capacity(balance, Some(limit)), balance);
    }

    #[test]
    fn test_effective_capacity_zero_limit() {
        let balance = U256::from(1_000_000u64);
        assert_eq!(effective_capacity(balance, Some(U256::ZERO)), U256::ZERO);
    }

    #[test]
    fn test_effective_capacity_zero_balance() {
        let limit = U256::from(1_000_000u64);
        assert_eq!(effective_capacity(U256::ZERO, Some(limit)), U256::ZERO);
    }

    #[test]
    fn test_effective_capacity_both_zero() {
        assert_eq!(effective_capacity(U256::ZERO, Some(U256::ZERO)), U256::ZERO);
    }
}
