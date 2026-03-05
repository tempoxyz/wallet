//! Payment dispatch: route 402 flows to charge or session payment paths.
//!
//! This module is crate-internal and intentionally decoupled from CLI types.

use anyhow::Result;
use mpp::protocol::methods::tempo::session::TempoSessionExt;
use mpp::protocol::methods::tempo::TempoChargeExt;

use mpp::PaymentChallenge;

use crate::config::Config;
use crate::error::TempoWalletError;
use crate::http::{HttpClient, HttpResponse};
use crate::keys::Keystore;
use crate::network::NetworkId;
use alloy::primitives::utils::format_units;

use super::charge::handle_charge_request;
use super::session::handle_session_request;

/// Parsed challenge with resolved network, shared by charge and session flows.
pub(super) struct ResolvedChallenge {
    pub challenge: PaymentChallenge,
    pub network_id: NetworkId,
    pub rpc_url: url::Url,
}

/// Result of a successful payment dispatch.
pub(crate) struct PaymentResult {
    pub tx_hash: String,
    pub session_id: Option<String>,
    pub status_code: u16,
    pub response: Option<HttpResponse>,
}

/// Dispatch to charge or session payment flow.
pub(crate) async fn dispatch_payment(
    config: &Config,
    http: &HttpClient,
    is_session: bool,
    url: &str,
    challenge: PaymentChallenge,
    keys: &Keystore,
) -> Result<PaymentResult> {
    let chain_id = if challenge.intent.is_charge() {
        challenge
            .request
            .decode::<mpp::ChargeRequest>()
            .ok()
            .and_then(|r| r.chain_id())
    } else if challenge.intent.is_session() {
        challenge
            .request
            .decode::<mpp::SessionRequest>()
            .ok()
            .and_then(|r| r.chain_id())
    } else {
        None
    };

    let chain_id = chain_id.ok_or_else(|| {
        TempoWalletError::InvalidConfig("Missing chainId in payment request".to_string())
    })?;

    let network_id = NetworkId::require_chain_id(chain_id)?;

    if let Some(allowed) = http.network {
        anyhow::ensure!(
            allowed == network_id,
            "Server requested network '{}' but --network is '{}'",
            network_id,
            allowed
        );
    }

    let rpc_url = config.rpc_url(network_id);
    let resolved = ResolvedChallenge {
        challenge,
        network_id,
        rpc_url,
    };

    let signer = keys.signer(resolved.network_id)?;

    if is_session {
        return handle_session_request(http, url, resolved, signer, keys).await;
    }

    handle_charge_request(http, url, resolved, signer).await
}

/// Map mpp validation errors to  tempo-walleterror types.
pub(super) fn map_mpp_validation_error(
    e: mpp::MppError,
    challenge: &mpp::PaymentChallenge,
) -> TempoWalletError {
    match e {
        mpp::MppError::UnsupportedPaymentMethod(msg) => {
            TempoWalletError::UnsupportedPaymentMethod(msg)
        }
        mpp::MppError::PaymentExpired(_) => {
            TempoWalletError::ChallengeExpired(challenge.expires.clone().unwrap_or_default())
        }
        mpp::MppError::InvalidChallenge { reason, .. } => {
            TempoWalletError::UnsupportedPaymentIntent(reason.unwrap_or_default())
        }
        other => TempoWalletError::InvalidChallenge(other.to_string()),
    }
}

/// Classify an mpp provider error into a TempoWalletError with actionable context.
pub(super) fn classify_payment_error(err: mpp::MppError, network: &NetworkId) -> TempoWalletError {
    use mpp::client::TempoClientError;

    match err {
        mpp::MppError::Tempo(tempo_err) => match tempo_err {
            TempoClientError::AccessKeyNotProvisioned => {
                TempoWalletError::AccessKeyNotProvisioned {
                    hint: " tempo-walletlogin".to_string(),
                }
            }
            TempoClientError::SpendingLimitExceeded {
                token,
                limit,
                required,
            } => TempoWalletError::SpendingLimitExceeded {
                token,
                limit,
                required,
            },
            TempoClientError::InsufficientBalance {
                token,
                available,
                required,
            } => {
                let tc = network.token();
                let (symbol, decimals) = if tc.address.eq_ignore_ascii_case(&token) {
                    (tc.symbol, tc.decimals)
                } else {
                    ("tokens", 6)
                };
                let fmt = |s: &str| {
                    s.parse::<u128>()
                        .ok()
                        .and_then(|v| format_units(v, decimals).ok())
                        .unwrap_or_else(|| s.to_string())
                };
                let avail_fmt = fmt(&available);
                let req_fmt = fmt(&required);
                TempoWalletError::InsufficientBalance {
                    token: symbol.to_string(),
                    available: avail_fmt,
                    required: req_fmt,
                }
            }
            TempoClientError::TransactionReverted(msg) => TempoWalletError::Http(msg),
        },
        other => {
            let raw = other.to_string();
            let msg = raw.strip_prefix("HTTP error: ").unwrap_or(&raw).to_string();
            TempoWalletError::Http(msg)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_spending_limit() {
        let err = mpp::MppError::Tempo(mpp::client::TempoClientError::SpendingLimitExceeded {
            token: "pathUSD".to_string(),
            limit: "0.000000".to_string(),
            required: "0.010000".to_string(),
        });
        match classify_payment_error(err, &NetworkId::TempoModerato) {
            TempoWalletError::SpendingLimitExceeded {
                token,
                limit,
                required,
            } => {
                assert_eq!(token, "pathUSD");
                assert_eq!(limit, "0.000000");
                assert_eq!(required, "0.010000");
            }
            other => panic!("Expected SpendingLimitExceeded, got: {other}"),
        }
    }

    #[test]
    fn test_classify_insufficient_balance_non_address() {
        let err = mpp::MppError::Tempo(mpp::client::TempoClientError::InsufficientBalance {
            token: "pathUSD".to_string(),
            available: "0.50".to_string(),
            required: "1.00".to_string(),
        });
        match classify_payment_error(err, &NetworkId::TempoModerato) {
            TempoWalletError::InsufficientBalance {
                token,
                available,
                required,
            } => {
                // "pathUSD" is not an address, so falls back to "tokens"
                assert_eq!(token, "tokens");
                assert_eq!(available, "0.50");
                assert_eq!(required, "1.00");
            }
            other => panic!("Expected InsufficientBalance, got: {other}"),
        }
    }

    #[test]
    fn test_classify_key_not_provisioned() {
        let err = mpp::MppError::Tempo(mpp::client::TempoClientError::AccessKeyNotProvisioned);
        assert!(matches!(
            classify_payment_error(err, &NetworkId::Tempo),
            TempoWalletError::AccessKeyNotProvisioned { .. }
        ));
    }

    #[test]
    fn test_classify_insufficient_balance_usdc_address() {
        let err = mpp::MppError::Tempo(mpp::client::TempoClientError::InsufficientBalance {
            token: "0x20c000000000000000000000b9537d11c60e8b50".to_string(),
            available: "0".to_string(),
            required: "1000".to_string(),
        });
        match classify_payment_error(err, &NetworkId::Tempo) {
            TempoWalletError::InsufficientBalance {
                token,
                available,
                required,
            } => {
                assert_eq!(token, "USDC");
                assert_eq!(available, "0.000000");
                assert_eq!(required, "0.001000");
            }
            other => panic!("Expected InsufficientBalance, got: {other}"),
        }
    }

    #[test]
    fn test_classify_insufficient_balance_pathusd_address() {
        let err = mpp::MppError::Tempo(mpp::client::TempoClientError::InsufficientBalance {
            token: "0x20c0000000000000000000000000000000000000".to_string(),
            available: "500000".to_string(),
            required: "1000000".to_string(),
        });
        match classify_payment_error(err, &NetworkId::TempoModerato) {
            TempoWalletError::InsufficientBalance {
                token,
                available,
                required,
            } => {
                assert_eq!(token, "pathUSD");
                assert_eq!(available, "0.500000");
                assert_eq!(required, "1.000000");
            }
            other => panic!("Expected InsufficientBalance, got: {other}"),
        }
    }

    #[test]
    fn test_classify_unrecognized_falls_through() {
        let err = mpp::MppError::Http("something unexpected".to_string());
        match classify_payment_error(err, &NetworkId::Tempo) {
            TempoWalletError::Http(msg) => assert_eq!(msg, "something unexpected"),
            other => panic!("Expected Http passthrough, got: {other}"),
        }
    }
}
