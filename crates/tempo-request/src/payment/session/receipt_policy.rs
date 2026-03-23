use super::ChannelState;
use tempo_common::{
    cli::terminal::sanitize_for_terminal,
    error::{PaymentError, TempoError},
};

pub(super) fn warn_missing_payment_receipt(context: &str) {
    eprintln!("Warning: missing Payment-Receipt on successful paid {context}");
}

pub(super) fn warn_invalid_payment_receipt(context: &str, reason: &str) {
    let safe_reason = sanitize_for_terminal(reason);
    eprintln!("Warning: ignoring invalid Payment-Receipt on paid {context}: {safe_reason}");
}

pub(super) fn missing_payment_receipt_error(context: &str) -> TempoError {
    PaymentError::PaymentRejected {
        reason: format!("Missing required Payment-Receipt on successful paid {context}"),
        status_code: 502,
    }
    .into()
}

pub(super) fn invalid_payment_receipt_error(context: &str, reason: &str) -> TempoError {
    let safe_reason = sanitize_for_terminal(reason);
    PaymentError::PaymentRejected {
        reason: format!(
            "Invalid required Payment-Receipt on successful paid {context}: {safe_reason}"
        ),
        status_code: 502,
    }
    .into()
}

pub(super) fn protocol_spent_error(reason: String) -> TempoError {
    PaymentError::PaymentRejected {
        reason: format!("Malformed payment protocol field: payment-receipt.spent {reason}"),
        status_code: 502,
    }
    .into()
}

pub(super) fn apply_receipt_amounts(
    state: &mut ChannelState,
    accepted_cumulative: u128,
    spent: Option<u128>,
) {
    state.cumulative_amount = state.cumulative_amount.max(accepted_cumulative);
    state.accepted_cumulative = state.accepted_cumulative.max(accepted_cumulative);

    if let Some(spent) = spent {
        if spent > 0 && spent <= accepted_cumulative {
            state.server_spent = spent;
        }
    }
}

pub(super) fn apply_receipt_amounts_strict(
    state: &mut ChannelState,
    accepted_cumulative: u128,
    spent: Option<u128>,
) -> Result<(), TempoError> {
    let spent = spent.ok_or_else(|| protocol_spent_error("is missing".to_string()))?;

    if spent > accepted_cumulative {
        return Err(protocol_spent_error(format!(
            "must be <= acceptedCumulative (spent={spent}, acceptedCumulative={accepted_cumulative})"
        )));
    }

    state.cumulative_amount = state.cumulative_amount.max(accepted_cumulative);
    state.accepted_cumulative = state.accepted_cumulative.max(accepted_cumulative);
    state.server_spent = spent;
    Ok(())
}

#[cfg(test)]
mod tests {
    use alloy::primitives::{Address, B256};

    use super::{
        apply_receipt_amounts, apply_receipt_amounts_strict, invalid_payment_receipt_error,
        missing_payment_receipt_error,
    };
    use crate::payment::session::ChannelState;

    fn state() -> ChannelState {
        ChannelState {
            channel_id: B256::from([0x11; 32]),
            escrow_contract: Address::ZERO,
            chain_id: 4217,
            deposit: 100,
            cumulative_amount: 20,
            accepted_cumulative: 10,
            server_spent: 5,
            strict_receipts: true,
        }
    }

    #[test]
    fn apply_receipt_amounts_is_monotonic_and_bounds_spent() {
        let mut state = state();

        apply_receipt_amounts(&mut state, 25, Some(9));
        assert_eq!(state.cumulative_amount, 25);
        assert_eq!(state.accepted_cumulative, 25);
        assert_eq!(state.server_spent, 9);

        apply_receipt_amounts(&mut state, 15, Some(16));
        assert_eq!(
            state.cumulative_amount, 25,
            "cumulative should not decrease"
        );
        assert_eq!(
            state.accepted_cumulative, 25,
            "accepted should not decrease"
        );
        assert_eq!(
            state.server_spent, 9,
            "spent above accepted cumulative should be ignored in permissive mode"
        );
    }

    #[test]
    fn apply_receipt_amounts_strict_requires_spent() {
        let mut state = state();
        let err = apply_receipt_amounts_strict(&mut state, 25, None).unwrap_err();
        assert!(err.to_string().contains("payment-receipt.spent is missing"));
    }

    #[test]
    fn apply_receipt_amounts_strict_rejects_spent_above_accepted() {
        let mut state = state();
        let err = apply_receipt_amounts_strict(&mut state, 25, Some(30)).unwrap_err();
        assert!(err.to_string().contains("must be <= acceptedCumulative"));
    }

    #[test]
    fn apply_receipt_amounts_strict_allows_reconciled_lower_spent() {
        let mut state = state();
        state.server_spent = 10;

        let result = apply_receipt_amounts_strict(&mut state, 30, Some(7));
        assert!(result.is_ok());
        assert_eq!(state.server_spent, 7);
        assert_eq!(state.accepted_cumulative, 30);
    }

    #[test]
    fn invalid_payment_receipt_error_sanitizes_control_chars() {
        let err = invalid_payment_receipt_error("session response", "bad\u{1b}[31m");
        let msg = err.to_string();
        assert!(!msg.chars().any(char::is_control));
        assert!(msg.contains("bad[31m"));
    }

    #[test]
    fn missing_payment_receipt_error_mentions_context() {
        let err = missing_payment_receipt_error("topUp response");
        assert!(err.to_string().contains("topUp response"));
    }
}
