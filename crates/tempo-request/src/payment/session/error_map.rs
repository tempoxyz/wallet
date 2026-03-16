use tempo_common::{
    cli::terminal::sanitize_for_terminal,
    error::{PaymentError, TempoError},
};

const MAX_REJECTED_REASON_CHARS: usize = 500;

pub(super) fn payment_rejected_reason_from_body(body: &str) -> String {
    let raw_reason = tempo_common::payment::extract_json_error(body).unwrap_or_else(|| {
        body.chars()
            .take(MAX_REJECTED_REASON_CHARS)
            .collect::<String>()
    });
    sanitize_for_terminal(&raw_reason)
}

pub(super) fn payment_rejected_from_body(status_code: u16, body: &str) -> TempoError {
    PaymentError::PaymentRejected {
        reason: payment_rejected_reason_from_body(body),
        status_code,
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::payment_rejected_from_body;
    use tempo_common::error::{PaymentError, TempoError};

    #[test]
    fn payment_rejected_from_body_maps_common_payload_forms() {
        let oversized = "x".repeat(600);
        let malformed = "{\"error\":\"unterminated";
        let cases = vec![
            (
                "json_problem",
                410,
                r#"{"type":"https://paymentauth.org/problems/session/channel-not-found","detail":"bad\u001b[31m\u0007value"}"#
                    .to_string(),
            ),
            (
                "json_error",
                402,
                r#"{"error":"bad\u001b[31m\u0007value"}"#.to_string(),
            ),
            ("plaintext", 500, "plain failure".to_string()),
            ("oversized", 400, oversized),
            ("malformed", 400, malformed.to_string()),
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
                        "json_problem" => {
                            assert_eq!(
                                reason,
                                "https://paymentauth.org/problems/session/channel-not-found: bad[31mvalue"
                            );
                        }
                        "json_error" => assert_eq!(reason, "bad[31mvalue"),
                        "plaintext" => assert_eq!(reason, "plain failure"),
                        "oversized" => assert_eq!(reason.len(), 500),
                        "malformed" => assert_eq!(reason, malformed),
                        _ => unreachable!("unexpected case"),
                    }
                }
                other => panic!("unexpected error variant for case={name}: {other:?}"),
            }
        }
    }
}
