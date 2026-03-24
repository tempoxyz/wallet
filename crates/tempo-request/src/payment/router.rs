//! Payment routing: route 402 flows to charge or session payment paths.
//!
//! This module is crate-internal and intentionally decoupled from CLI types.

use mpp::{client::TempoClientError, PaymentChallenge};
use serde::Deserialize;

use crate::http::HttpClient;
use tempo_common::{
    config::Config,
    error::{PaymentError, TempoError},
    keys::Keystore,
    network::NetworkId,
};

use super::{
    charge::handle_charge_request,
    session::handle_session_request,
    types::{PaymentResult, ResolvedChallenge},
};
use tempo_common::payment::classify_payment_error;

/// Dispatch to charge or session payment flow.
///
/// `network` is the already-resolved network from the 402 challenge.
/// The caller is responsible for parsing the challenge and extracting
/// the network before calling this function (see `query/challenge.rs`).
#[allow(clippy::too_many_arguments)]
pub(crate) async fn dispatch_payment(
    config: &Config,
    http: &HttpClient,
    is_session: bool,
    url: &str,
    challenge: PaymentChallenge,
    network: NetworkId,
    keys: &Keystore,
) -> Result<PaymentResult, TempoError> {
    if let Some(allowed) = http.network {
        if allowed != network {
            return Err(PaymentError::ChallengeSchema {
                context: "payment challenge network",
                reason: format!(
                    "Server requested network '{network}' but --network is '{allowed}'"
                ),
            }
            .into());
        }
    }

    if let Some(mock_error) = mock_payment_error_from_env(network) {
        return Err(mock_error);
    }

    let rpc_url = config.rpc_url(network);
    let resolved = ResolvedChallenge {
        challenge,
        network_id: network,
        rpc_url,
    };

    let signer = keys.signer(resolved.network_id)?;

    if is_session {
        return handle_session_request(http, url, resolved, signer, keys).await;
    }

    handle_charge_request(http, url, resolved, signer).await
}

fn mock_payment_error_from_env(network: NetworkId) -> Option<TempoError> {
    let value = std::env::var("TEMPO_MOCK_PAYMENT_ERROR").ok()?;
    parse_mock_payment_error(value.trim(), network)
}

fn parse_mock_payment_error(value: &str, network: NetworkId) -> Option<TempoError> {
    let err = if value.starts_with('{') {
        parse_mock_payment_error_json(value, network)
    } else if value.eq_ignore_ascii_case("insufficient-balance") {
        Some(classify_payment_error(
            mpp::MppError::Tempo(TempoClientError::InsufficientBalance {
                token: format!("{:#x}", network.token().address),
                available: "0".to_string(),
                required: "1000000".to_string(),
            }),
            &network,
        ))
    } else if value.eq_ignore_ascii_case("spending-limit") {
        Some(classify_payment_error(
            mpp::MppError::Tempo(TempoClientError::SpendingLimitExceeded {
                token: network.token().symbol.to_string(),
                limit: "0.000000".to_string(),
                required: "1.000000".to_string(),
            }),
            &network,
        ))
    } else {
        tracing::warn!(
            "Ignoring unknown TEMPO_MOCK_PAYMENT_ERROR value '{value}'. Expected one of: insufficient-balance, spending-limit, or a JSON object"
        );
        None
    };

    err
}

#[derive(Deserialize)]
struct MockTempoErrorPayload {
    #[serde(rename = "type")]
    kind: String,
    token: Option<String>,
    available: Option<String>,
    required: Option<String>,
    limit: Option<String>,
}

fn parse_mock_payment_error_json(value: &str, network: NetworkId) -> Option<TempoError> {
    let payload: MockTempoErrorPayload = serde_json::from_str(value).ok()?;

    let err = if payload.kind.eq_ignore_ascii_case("insufficient-balance") {
        TempoClientError::InsufficientBalance {
            token: payload
                .token
                .unwrap_or_else(|| format!("{:#x}", network.token().address)),
            available: payload.available.unwrap_or_else(|| "0".to_string()),
            required: payload.required.unwrap_or_else(|| "1000000".to_string()),
        }
    } else if payload.kind.eq_ignore_ascii_case("spending-limit") {
        TempoClientError::SpendingLimitExceeded {
            token: payload
                .token
                .unwrap_or_else(|| network.token().symbol.to_string()),
            limit: payload.limit.unwrap_or_else(|| "0.000000".to_string()),
            required: payload.required.unwrap_or_else(|| "1.000000".to_string()),
        }
    } else {
        tracing::warn!(
            "Ignoring unsupported TEMPO_MOCK_PAYMENT_ERROR JSON type '{}'. Expected insufficient-balance or spending-limit",
            payload.kind
        );
        return None;
    };

    Some(classify_payment_error(mpp::MppError::Tempo(err), &network))
}

#[cfg(test)]
mod tests {
    use super::parse_mock_payment_error;
    use tempo_common::{
        error::{PaymentError, TempoError},
        network::NetworkId,
    };

    #[test]
    fn parse_mock_insufficient_balance() {
        let err = parse_mock_payment_error("insufficient-balance", NetworkId::Tempo)
            .expect("mock error should parse");
        assert!(matches!(
            err,
            TempoError::Payment(PaymentError::InsufficientBalance { .. })
        ));
    }

    #[test]
    fn parse_mock_spending_limit() {
        let err = parse_mock_payment_error("spending-limit", NetworkId::TempoModerato)
            .expect("mock error should parse");
        assert!(matches!(
            err,
            TempoError::Payment(PaymentError::SpendingLimitExceeded { .. })
        ));
    }

    #[test]
    fn parse_mock_unknown_value_returns_none() {
        let err = parse_mock_payment_error("wat", NetworkId::Tempo);
        assert!(err.is_none());
    }

    #[test]
    fn parse_mock_json_insufficient_balance_uses_classification_path() {
        let err = parse_mock_payment_error(
            r#"{"type":"insufficient-balance","token":"0x20c000000000000000000000b9537d11c60e8b50","available":"0","required":"1000000"}"#,
            NetworkId::Tempo,
        )
        .expect("mock error should parse");

        match err {
            TempoError::Payment(PaymentError::InsufficientBalance {
                token,
                available,
                required,
            }) => {
                assert_eq!(token, "USDC");
                assert_eq!(available, "0.000000");
                assert_eq!(required, "1.000000");
            }
            other => panic!("expected classified InsufficientBalance, got: {other}"),
        }
    }

    #[test]
    fn parse_mock_invalid_json_returns_none() {
        let err = parse_mock_payment_error("{bad-json", NetworkId::Tempo);
        assert!(err.is_none());
    }
}
