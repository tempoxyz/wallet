//! Payment provider abstraction for presto.
//!
//! This module provides payment providers that implement the mpp::client::PaymentProvider trait,
//! enabling automatic MPP (https://mpp.sh) handling with presto-specific features like keychain signing.

use crate::config::Config;
use crate::error::{PrestoError, Result};
use crate::network::Network;
use mpp::format_u256_with_decimals;
use mpp::client::tempo::keychain::{local_key_spending_limit, query_key_spending_limit};
use mpp::PaymentChallenge;
use mpp::client::tempo::swap::{SwapInfo, SWAP_SLIPPAGE_BPS};
#[cfg(test)]
use crate::payment::currency::Money;
use crate::wallet::signer::load_signer_for_network;
use alloy::primitives::{Address, U256};
use alloy::providers::ProviderBuilder;
use std::str::FromStr;
use std::sync::Arc;
use tempo_primitives::transaction::SignedKeyAuthorization;
use tracing::debug;

/// Presto payment provider that wraps config and implements mpp::client::PaymentProvider.
///
/// This provider handles both Tempo and EVM networks, automatically selecting
/// the appropriate transaction format based on the payment method.
///
#[derive(Clone)]
pub struct PrestoPaymentProvider {
    config: Arc<Config>,
}

impl PrestoPaymentProvider {
    /// Create a new provider with the given configuration.
    pub fn new(config: Config) -> Self {
        Self {
            config: Arc::new(config),
        }
    }
}

impl mpp::client::PaymentProvider for PrestoPaymentProvider {
    fn supports(&self, method: &str, intent: &str) -> bool {
        let method_lower = method.to_lowercase();
        let is_supported_method = method_lower == "tempo";
        let is_charge = intent == "charge";
        is_supported_method && is_charge
    }

    async fn pay(
        &self,
        challenge: &mpp::PaymentChallenge,
    ) -> std::result::Result<mpp::PaymentCredential, mpp::MppError> {
        let method = challenge.method.as_str().to_lowercase();

        match method.as_str() {
            "tempo" => self.create_tempo_payment(challenge).await,
            _ => Err(mpp::MppError::UnsupportedPaymentMethod(format!(
                "Payment method '{}' is not supported",
                challenge.method
            ))),
        }
    }
}

impl PrestoPaymentProvider {
    async fn create_tempo_payment(
        &self,
        challenge: &mpp::PaymentChallenge,
    ) -> std::result::Result<mpp::PaymentCredential, mpp::MppError> {
        use mpp::protocol::methods::tempo::TempoChargeExt;
        use crate::payment::tempo::{
            create_tempo_payment, create_tempo_payment_with_swap,
        };

        let charge_req: mpp::ChargeRequest = challenge
            .request
            .decode()
            .map_err(|e| mpp::MppError::Http(format!("Invalid charge request: {}", e)))?;

        let required_token = charge_req
            .currency_address()
            .map_err(|e| mpp::MppError::Http(format!("Invalid currency address: {}", e)))?;
        let required_amount = charge_req
            .amount_u256()
            .map_err(|e| mpp::MppError::Http(format!("Invalid amount: {}", e)))?;

        let network = network_from_charge_request(&charge_req)
            .map_err(|e| mpp::MppError::Http(e.to_string()))?;
        let network_name = network.as_str();

        let signer_ctx = load_signer_for_network(network_name)
            .map_err(|e| mpp::MppError::Http(e.to_string()))?;

        let key_address = signer_ctx.signer.address();
        let wallet_address = signer_ctx
            .wallet_address
            .as_ref()
            .map(|addr| Address::from_str(addr))
            .transpose()
            .map_err(|e| mpp::MppError::Http(format!("Invalid wallet address: {}", e)))?;

        let local_auth = signer_ctx
            .key_authorization
            .as_deref()
            .and_then(crate::wallet::decode_key_authorization);

        let from = wallet_address.unwrap_or(key_address);

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
            .map_err(|e| mpp::MppError::Http(e.to_string()))?;

        debug!(
            balance = %format_u256_with_decimals(balance, token_decimals),
            token = %token_symbol,
            "queried wallet balance"
        );

        let spending_limit = if let Some(wallet_addr) = wallet_address {
            let network_info = self
                .config
                .resolve_network(network_name)
                .map_err(|e| mpp::MppError::Http(e.to_string()))?;
            let provider = ProviderBuilder::new().connect_http(
                network_info
                    .rpc_url
                    .parse()
                    .map_err(|e| mpp::MppError::Http(format!("Invalid RPC URL: {}", e)))?,
            );

            let limit =
                match query_key_spending_limit(&provider, wallet_addr, key_address, required_token)
                    .await
                {
                    Ok(limit) => limit,
                    Err(_) if local_auth.is_some() => {
                        local_key_spending_limit(local_auth.as_ref().unwrap(), required_token)
                    }
                    Err(e) => {
                        return Err(mpp::MppError::Http(format!(
                            "Cannot verify key spending limit for {}: {}",
                            token_symbol, e
                        )));
                    }
                };

            limit
        } else {
            None
        };

        let effective_capacity = mpp::client::tempo::effective_capacity(balance, spending_limit);

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
                .map_err(|e| {
                    classify_payment_error(
                        e,
                        &token_symbol,
                        token_decimals,
                        balance,
                        spending_limit,
                        required_amount,
                    )
                });
        }

        let limit_is_bottleneck = spending_limit
            .map(|limit| limit < required_amount)
            .unwrap_or(false);

        if limit_is_bottleneck {
            let limit_human =
                format_u256_with_decimals(spending_limit.unwrap_or(U256::ZERO), token_decimals);
            let needed_human = format_u256_with_decimals(required_amount, token_decimals);
            return Err(mpp::client::TempoClientError::SpendingLimitExceeded {
                token: token_symbol.clone(),
                limit: limit_human,
                required: needed_human,
            }
            .into());
        }

        let keychain_info = wallet_address.map(|wa| (wa, key_address));
        let swap_source = find_swap_source(
            &self.config,
            network,
            from,
            required_token,
            required_amount,
            keychain_info,
            local_auth.as_ref(),
        )
        .await
        .map_err(|e| mpp::MppError::Http(e.to_string()))?;

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
                    .map_err(|e| {
                        classify_payment_error(
                            e,
                            &token_symbol,
                            token_decimals,
                            balance,
                            spending_limit,
                            required_amount,
                        )
                    })
            }
            None => Err(mpp::client::TempoClientError::InsufficientBalance {
                token: token_symbol.clone(),
                available: format_u256_with_decimals(balance, token_decimals),
                required: format_u256_with_decimals(required_amount, token_decimals),
            }
            .into()),
        }
    }
}

/// Classify a payment transaction error into a specific MppError.
///
/// Matches on typed mpp error variants that propagate through from gas estimation
/// and signing. Enriches revert errors with token context (symbol, decimals).
fn classify_payment_error(
    err: PrestoError,
    token_symbol: &str,
    token_decimals: u8,
    balance: U256,
    spending_limit: Option<U256>,
    required_amount: U256,
) -> mpp::MppError {
    use mpp::client::TempoClientError;

    match err {
        PrestoError::Mpp(mpp::MppError::Tempo(ref tempo_err)) => match tempo_err {
            TempoClientError::AccessKeyNotProvisioned => {
                mpp::MppError::from(TempoClientError::AccessKeyNotProvisioned)
            }
            TempoClientError::TransactionReverted(revert_msg) => {
                let lower = revert_msg.to_lowercase();
                if lower.contains("spendinglimitexceeded") || lower.contains("spending limit") {
                    let limit_value = spending_limit.unwrap_or(balance);
                    TempoClientError::SpendingLimitExceeded {
                        token: token_symbol.to_string(),
                        limit: format_u256_with_decimals(limit_value, token_decimals),
                        required: format_u256_with_decimals(required_amount, token_decimals),
                    }
                    .into()
                } else {
                    TempoClientError::InsufficientBalance {
                        token: token_symbol.to_string(),
                        available: format_u256_with_decimals(balance, token_decimals),
                        required: format_u256_with_decimals(required_amount, token_decimals),
                    }
                    .into()
                }
            }
            _ => mpp::MppError::Http(err.to_string()),
        },
        other => mpp::MppError::Http(other.to_string()),
    }
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
            PrestoError::InvalidConfig(format!("Invalid RPC URL for {network}: {e}"))
        })?);

    mpp::client::tempo::query_token_balance(&provider, token_address, account)
        .await
        .map_err(|e| PrestoError::BalanceQuery(format!("Failed to query balance: {}", e)))
}

/// Find a token with sufficient balance (and spending limit) to swap from.
///
/// Builds the candidate list from the network's supported tokens, constructs
/// a provider from config, and delegates to `mpp::client::tempo::find_swap_source`.
pub async fn find_swap_source(
    config: &Config,
    network: Network,
    account: Address,
    required_token: Address,
    required_amount: U256,
    keychain_info: Option<(Address, Address)>,
    local_auth: Option<&SignedKeyAuthorization>,
) -> Result<Option<mpp::client::tempo::SwapSource>> {
    use mpp::client::tempo::routing::SwapCandidate;

    let network_info = config.resolve_network(network.as_str())?;
    let provider =
        ProviderBuilder::new().connect_http(network_info.rpc_url.parse().map_err(|e| {
            PrestoError::InvalidConfig(format!("Invalid RPC URL for {network}: {e}"))
        })?);

    let candidates: Vec<_> = network
        .supported_tokens()
        .into_iter()
        .filter_map(|token_config| {
            let token_address = Address::from_str(token_config.address).ok()?;
            Some(SwapCandidate {
                address: token_address,
                symbol: token_config.currency.symbol.to_string(),
            })
        })
        .collect();

    mpp::client::tempo::find_swap_source(
        &provider,
        account,
        required_token,
        required_amount,
        &candidates,
        keychain_info,
        local_auth,
    )
    .await
    .map_err(|e| PrestoError::InvalidConfig(e.to_string()))
}

/// Derive the network from a charge request's chain ID.
pub fn network_from_charge_request(req: &mpp::ChargeRequest) -> crate::error::Result<Network> {
    use mpp::protocol::methods::tempo::TempoChargeExt;
    let chain_id = req.chain_id().ok_or_else(|| {
        crate::error::PrestoError::InvalidConfig("Missing chainId in charge request".to_string())
    })?;
    Network::from_chain_id(chain_id).ok_or_else(|| {
        crate::error::PrestoError::InvalidConfig(format!("Unsupported chainId: {}", chain_id))
    })
}

/// Derive the network from a session request's chain ID.
pub fn network_from_session_request(req: &mpp::SessionRequest) -> crate::error::Result<Network> {
    use mpp::protocol::methods::tempo::session::TempoSessionExt;
    let chain_id = req.chain_id().ok_or_else(|| {
        crate::error::PrestoError::InvalidConfig("Missing chainId in session request".to_string())
    })?;
    Network::from_chain_id(chain_id).ok_or_else(|| {
        crate::error::PrestoError::InvalidConfig(format!("Unsupported chainId: {}", chain_id))
    })
}

/// Validate that a payment challenge can be processed by presto's charge flow.
///
/// Delegates to `PaymentChallenge::validate_for_charge("tempo")` from mpp,
/// mapping mpp errors to presto error types.
pub fn validate_challenge(challenge: &PaymentChallenge) -> Result<()> {
    challenge.validate_for_charge("tempo").map_err(|e| match e {
        mpp::MppError::UnsupportedPaymentMethod(msg) => PrestoError::UnsupportedPaymentMethod(msg),
        mpp::MppError::PaymentExpired(_) => {
            PrestoError::ChallengeExpired(challenge.expires.clone().unwrap_or_default())
        }
        mpp::MppError::InvalidChallenge { reason, .. } => {
            PrestoError::UnsupportedPaymentIntent(reason.unwrap_or_default())
        }
        other => PrestoError::InvalidChallenge(other.to_string()),
    })
}

/// Validate that a payment challenge is a valid session challenge.
///
/// Delegates to `PaymentChallenge::validate_for_session("tempo")` from mpp,
/// mapping mpp errors to presto error types.
pub fn validate_session_challenge(challenge: &PaymentChallenge) -> Result<()> {
    challenge
        .validate_for_session("tempo")
        .map_err(|e| match e {
            mpp::MppError::UnsupportedPaymentMethod(msg) => {
                PrestoError::UnsupportedPaymentMethod(msg)
            }
            mpp::MppError::PaymentExpired(_) => {
                PrestoError::ChallengeExpired(challenge.expires.clone().unwrap_or_default())
            }
            mpp::MppError::InvalidChallenge { reason, .. } => {
                PrestoError::UnsupportedPaymentIntent(reason.unwrap_or_default())
            }
            other => PrestoError::InvalidChallenge(other.to_string()),
        })
}

/// Presto-specific extensions to ChargeRequest.
///
/// For core EVM accessors (including `memo()`), use `TempoChargeExt` from mpp.
#[cfg(test)]
pub trait ChargeRequestExt {
    /// Create a type-safe `Money` value from this charge request.
    ///
    /// Validates that the currency address matches the network's configured token.
    fn money(&self, network: Network) -> Result<Money>;
}

#[cfg(test)]
impl ChargeRequestExt for mpp::ChargeRequest {
    fn money(&self, network: Network) -> Result<Money> {
        use crate::payment::currency::{Money, TokenId};
        use mpp::protocol::methods::tempo::TempoChargeExt;

        let currency_addr: Address = self
            .currency_address()
            .map_err(|e| PrestoError::InvalidAddress(e.to_string()))?;

        let token_config = network.require_token_config(&self.currency)?;

        let amount: U256 = self
            .amount_u256()
            .map_err(|e| PrestoError::InvalidAmount(e.to_string()))?;
        let token = TokenId::new(network, currency_addr);

        Ok(Money::new(
            token,
            amount,
            token_config.currency.decimals,
            token_config.currency.symbol,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mpp::client::PaymentProvider;

    #[test]
    fn test_provider_supports_tempo() {
        let config = Config::default();
        let provider = PrestoPaymentProvider::new(config);

        assert!(provider.supports("tempo", "charge"));
        assert!(provider.supports("TEMPO", "charge"));
        assert!(!provider.supports("tempo", "authorize"));
        assert!(!provider.supports("bitcoin", "charge"));
    }

    #[test]
    fn test_provider_rejects_unknown_methods() {
        let config = Config::default();
        let provider = PrestoPaymentProvider::new(config);

        assert!(!provider.supports("base", "charge"));
        assert!(!provider.supports("ethereum", "charge"));
        assert!(!provider.supports("bitcoin", "charge"));
        assert!(!provider.supports("unknown", "charge"));
    }

    // effective_capacity tests are in mpp-rs (client::tempo::balance)

    #[test]
    fn test_validate_challenge_valid() {
        use mpp::Base64UrlJson;
        let challenge = PaymentChallenge {
            id: "test".to_string(),
            realm: "test.example.com".to_string(),
            method: mpp::MethodName::new("tempo"),
            intent: "charge".into(),
            request: Base64UrlJson::from_value(&serde_json::json!({})).unwrap(),
            digest: None,
            description: None,
            expires: None,
        };
        assert!(validate_challenge(&challenge).is_ok());
    }

    #[test]
    fn test_validate_challenge_unsupported_method() {
        use mpp::Base64UrlJson;
        let challenge = PaymentChallenge {
            id: "test".to_string(),
            realm: "test.example.com".to_string(),
            method: mpp::MethodName::new("bitcoin"),
            intent: "charge".into(),
            request: Base64UrlJson::from_value(&serde_json::json!({})).unwrap(),
            digest: None,
            description: None,
            expires: None,
        };
        assert!(validate_challenge(&challenge).is_err());
    }

    #[test]
    fn test_validate_session_challenge_valid() {
        use mpp::Base64UrlJson;
        let challenge = PaymentChallenge {
            id: "test".to_string(),
            realm: "test.example.com".to_string(),
            method: mpp::MethodName::new("tempo"),
            intent: "session".into(),
            request: Base64UrlJson::from_value(&serde_json::json!({})).unwrap(),
            digest: None,
            description: None,
            expires: None,
        };
        assert!(validate_session_challenge(&challenge).is_ok());
    }

    #[test]
    fn test_validate_session_challenge_wrong_intent() {
        use mpp::Base64UrlJson;
        let challenge = PaymentChallenge {
            id: "test".to_string(),
            realm: "test.example.com".to_string(),
            method: mpp::MethodName::new("tempo"),
            intent: "charge".into(),
            request: Base64UrlJson::from_value(&serde_json::json!({})).unwrap(),
            digest: None,
            description: None,
            expires: None,
        };
        assert!(validate_session_challenge(&challenge).is_err());
    }

    #[test]
    fn test_charge_request_money() {
        let req = mpp::ChargeRequest {
            amount: "1000000".to_string(),
            currency: "0x20c0000000000000000000000000000000000000".to_string(),
            ..Default::default()
        };
        let money = req.money(Network::TempoModerato).expect("valid money");
        assert_eq!(money.atomic(), U256::from(1_000_000u64));
        assert_eq!(money.network(), Network::TempoModerato);
    }

    #[test]
    fn test_charge_request_money_wrong_currency() {
        let req = mpp::ChargeRequest {
            amount: "1000000".to_string(),
            currency: "0x1234567890123456789012345678901234567890".to_string(),
            ..Default::default()
        };
        assert!(req.money(Network::TempoModerato).is_err());
    }
}
