//! Payment routing: route 402 flows to charge or session payment paths.
//!
//! This module is crate-internal and intentionally decoupled from CLI types.

use mpp::PaymentChallenge;

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
    let token = network.token().symbol.to_string();

    let err = if value.eq_ignore_ascii_case("insufficient-balance") {
        Some(PaymentError::InsufficientBalance {
            token,
            available: "0.000000".to_string(),
            required: "1.000000".to_string(),
        })
    } else if value.eq_ignore_ascii_case("spending-limit") {
        Some(PaymentError::SpendingLimitExceeded {
            token,
            limit: "0.000000".to_string(),
            required: "1.000000".to_string(),
        })
    } else {
        tracing::warn!(
            "Ignoring unknown TEMPO_MOCK_PAYMENT_ERROR value '{value}'. Expected one of: insufficient-balance, spending-limit"
        );
        None
    };

    err.map(Into::into)
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
}
