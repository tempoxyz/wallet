use tempo_common::{
    cli::terminal::sanitize_for_terminal,
    error::{PaymentError, TempoError},
    payment::is_inactive_access_key_error,
};

const MAX_REJECTED_REASON_CHARS: usize = 500;

pub(super) fn payment_rejected_reason_from_body(body: &str) -> String {
    let raw_reason: String = if body.trim().is_empty() {
        "empty response".to_string()
    } else {
        body.chars().take(MAX_REJECTED_REASON_CHARS).collect()
    };
    sanitize_for_terminal(&raw_reason)
}

pub(super) fn payment_rejected_from_body(status_code: u16, body: &str) -> TempoError {
    let reason = payment_rejected_reason_from_body(body);
    if is_inactive_access_key_error(&reason) {
        return PaymentError::AccessKeyRevoked.into();
    }
    PaymentError::PaymentRejected {
        reason,
        status_code,
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::payment_rejected_from_body;
    use tempo_common::error::{PaymentError, TempoError};

    #[test]
    fn payment_rejected_from_body_returns_full_body() {
        let oversized = "x".repeat(600);
        let cases = vec![
            (
                "json_body",
                410,
                r#"{"type":"https://paymentauth.org/problems/session/channel-not-found","detail":"bad\u001b[31m\u0007value"}"#
                    .to_string(),
            ),
            ("plaintext", 500, "plain failure".to_string()),
            ("oversized", 400, oversized),
        ];

        for (name, status, body) in cases {
            let err = payment_rejected_from_body(status, &body);
            match err {
                TempoError::Payment(PaymentError::PaymentRejected {
                    reason,
                    status_code,
                }) => {
                    assert_eq!(status_code, status, "case={name}");
                    assert!(
                        !reason.chars().any(char::is_control),
                        "case={name} reason contained control bytes"
                    );
                    match name {
                        "json_body" => {
                            assert!(reason.contains("channel-not-found"), "case={name}");
                            assert!(reason.contains("bad"), "case={name}");
                        }
                        "plaintext" => assert_eq!(reason, "plain failure"),
                        "oversized" => assert_eq!(reason.len(), 500),
                        _ => unreachable!("unexpected case"),
                    }
                }
                other => panic!("unexpected error variant for case={name}: {other:?}"),
            }
        }
    }

    #[test]
    fn payment_rejected_from_body_maps_inactive_access_key_shape() {
        let body = r#"{"success":false,"error":"MPP payment failed: Payment verification failed: Missing or invalid parameters. URL: https://rpc.mainnet.tempo.xyz Request body: {\"method\":\"eth_sendRawTransactionSync\"}"}"#;
        let err = payment_rejected_from_body(402, body);
        assert!(matches!(
            err,
            TempoError::Payment(PaymentError::AccessKeyRevoked)
        ));
    }
}
