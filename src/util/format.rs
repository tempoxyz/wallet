//! Formatting helpers for token values and big integers.

/// Format a U256 value with the given number of decimal places.
///
/// Converts atomic units to a human-readable decimal string.
/// For example, `1000000` with 6 decimals becomes `"1.000000"`.
pub fn format_u256_with_decimals(value: alloy::primitives::U256, decimals: u8) -> String {
    use alloy::primitives::U256;

    if decimals == 0 {
        return value.to_string();
    }

    let divisor = U256::from(10u64).pow(U256::from(decimals));
    let whole = value / divisor;
    let remainder = value % divisor;

    let remainder_str = remainder.to_string();
    let padded = format!("{:0>width$}", remainder_str, width = decimals as usize);

    format!("{}.{}", whole, padded)
}

/// Format atomic token units as a human-readable string with trimmed trailing zeros.
pub fn format_token_amount(atomic: u128, symbol: &str, decimals: u8) -> String {
    let divisor = 10u128.pow(decimals as u32);
    let whole = atomic / divisor;
    let remainder = atomic % divisor;

    if remainder == 0 {
        format!("{whole} {symbol}")
    } else {
        let frac_str = format!("{:0width$}", remainder, width = decimals as usize);
        let trimmed = frac_str.trim_end_matches('0');
        format!("{whole}.{trimmed} {symbol}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_u256_zero() {
        use alloy::primitives::U256;
        assert_eq!(format_u256_with_decimals(U256::from(0), 6), "0.000000");
    }

    #[test]
    fn test_format_u256_zero_decimals() {
        use alloy::primitives::U256;
        assert_eq!(format_u256_with_decimals(U256::from(12345), 0), "12345");
    }

    #[test]
    fn test_format_u256_small_value() {
        use alloy::primitives::U256;
        assert_eq!(format_u256_with_decimals(U256::from(1), 6), "0.000001");
    }

    #[test]
    fn test_format_u256_exact_divisor() {
        use alloy::primitives::U256;
        assert_eq!(
            format_u256_with_decimals(U256::from(1_000_000u64), 6),
            "1.000000"
        );
    }

    #[test]
    fn test_format_u256_large_value() {
        use alloy::primitives::U256;
        assert_eq!(
            format_u256_with_decimals(U256::from(123_456_789u64), 6),
            "123.456789"
        );
    }

    #[test]
    fn test_format_u256_max() {
        use alloy::primitives::U256;
        let result = format_u256_with_decimals(U256::MAX, 18);
        assert!(result.contains('.'));
        assert!(!result.is_empty());
    }
}
