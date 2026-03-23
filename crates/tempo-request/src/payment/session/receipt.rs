use alloy::primitives::B256;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use mpp::{
    parse_receipt,
    protocol::{core::extract_tx_hash, methods::tempo::SessionReceipt},
};

#[derive(Debug, Clone)]
pub(super) struct ValidatedSessionReceipt {
    pub(super) accepted_cumulative: u128,
    pub(super) server_spent: Option<u128>,
    pub(super) spent_parse_error: Option<String>,
    pub(super) tx_reference: Option<String>,
}

pub(super) fn validate_session_receipt_fields(
    receipt: &SessionReceipt,
    expected_channel_id: B256,
) -> Result<u128, String> {
    if receipt.method != "tempo" {
        return Err(format!("method must be 'tempo' (got '{}')", receipt.method));
    }
    if receipt.intent != "session" {
        return Err(format!(
            "intent must be 'session' (got '{}')",
            receipt.intent
        ));
    }
    if receipt.status != "success" {
        return Err(format!(
            "status must be 'success' (got '{}')",
            receipt.status
        ));
    }

    let receipt_channel_id = receipt
        .channel_id
        .parse::<B256>()
        .map_err(|_| format!("invalid channelId bytes32 value: {}", receipt.channel_id))?;
    if receipt_channel_id != expected_channel_id {
        return Err(format!(
            "channelId mismatch (expected {expected_channel_id:#x}, got {receipt_channel_id:#x})"
        ));
    }

    receipt
        .accepted_cumulative
        .parse::<u128>()
        .map_err(|source| {
            format!(
                "acceptedCumulative must be an integer amount (got '{}'): {source}",
                receipt.accepted_cumulative
            )
        })
}

pub(super) fn parse_validated_session_receipt_header(
    receipt_header: &str,
    expected_channel_id: B256,
) -> Result<ValidatedSessionReceipt, String> {
    let base = parse_receipt(receipt_header)
        .map_err(|source| format!("invalid Payment-Receipt header: {source}"))?;
    if base.method.as_str() != "tempo" {
        return Err(format!(
            "method must be 'tempo' (got '{}')",
            base.method.as_str()
        ));
    }
    if !base.is_success() {
        return Err(format!("status must be 'success' (got '{}')", base.status));
    }

    let decoded = URL_SAFE_NO_PAD
        .decode(receipt_header.trim())
        .map_err(|source| format!("invalid Payment-Receipt base64url payload: {source}"))?;
    let session_receipt: SessionReceipt = serde_json::from_slice(&decoded)
        .map_err(|source| format!("invalid session receipt JSON payload: {source}"))?;

    let accepted_cumulative =
        validate_session_receipt_fields(&session_receipt, expected_channel_id)?;
    let (server_spent, spent_parse_error) = match session_receipt.spent.parse::<u128>() {
        Ok(value) => (Some(value), None),
        Err(source) => (
            None,
            Some(format!(
                "spent must be an integer amount (got '{}'): {source}",
                session_receipt.spent
            )),
        ),
    };
    let tx_reference = extract_tx_hash(receipt_header)
        .or_else(|| session_receipt.tx_hash.clone())
        .or(Some(base.reference));

    Ok(ValidatedSessionReceipt {
        accepted_cumulative,
        server_spent,
        spent_parse_error,
        tx_reference,
    })
}

#[cfg(test)]
mod tests {
    use super::{parse_validated_session_receipt_header, validate_session_receipt_fields};
    use alloy::primitives::B256;
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
    use mpp::protocol::methods::tempo::SessionReceipt;

    fn sample_receipt(channel_id: B256, accepted_cumulative: u128) -> SessionReceipt {
        SessionReceipt {
            method: "tempo".to_string(),
            intent: "session".to_string(),
            status: "success".to_string(),
            timestamp: "2026-01-01T00:00:00Z".to_string(),
            reference: format!("{channel_id:#x}"),
            challenge_id: "challenge-123".to_string(),
            channel_id: format!("{channel_id:#x}"),
            accepted_cumulative: accepted_cumulative.to_string(),
            spent: accepted_cumulative.to_string(),
            units: Some(7),
            tx_hash: Some(format!("{:#x}", B256::from([0x11; 32]))),
        }
    }

    fn encode_receipt_header(receipt: &SessionReceipt) -> String {
        let json = serde_json::to_vec(receipt).unwrap();
        URL_SAFE_NO_PAD.encode(json)
    }

    #[test]
    fn parse_validated_receipt_accepts_well_formed_header() {
        let channel_id = B256::from([0x22; 32]);
        let receipt = sample_receipt(channel_id, 42);
        let header = encode_receipt_header(&receipt);

        let parsed = parse_validated_session_receipt_header(&header, channel_id).unwrap();
        assert_eq!(parsed.accepted_cumulative, 42);
        assert!(parsed.tx_reference.is_some());
    }

    #[test]
    fn validate_session_receipt_rejects_wrong_channel_id() {
        let channel_id = B256::from([0x22; 32]);
        let wrong_channel = B256::from([0x33; 32]);
        let receipt = sample_receipt(wrong_channel, 42);

        let err = validate_session_receipt_fields(&receipt, channel_id).unwrap_err();
        assert!(err.contains("channelId mismatch"));
    }

    #[test]
    fn validate_session_receipt_rejects_wrong_intent() {
        let channel_id = B256::from([0x22; 32]);
        let mut receipt = sample_receipt(channel_id, 42);
        receipt.intent = "charge".to_string();

        let err = validate_session_receipt_fields(&receipt, channel_id).unwrap_err();
        assert!(err.contains("intent must be 'session'"));
    }
}
