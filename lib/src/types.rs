//! Type-safe newtypes for improved type safety.
//!
//! This module provides wrapper types that give stronger type guarantees
//! for domain-specific values like token amounts.

use alloy::primitives::U256;
use std::fmt;

/// A token amount with decimals information for proper formatting.
///
/// This newtype wraps a U256 atomic amount along with the decimals
/// required for human-readable formatting. It provides type safety
/// to prevent accidentally mixing amounts with different decimal
/// representations.
///
/// # Examples
///
/// ```
/// use purl::types::TokenAmount;
/// use alloy::primitives::U256;
///
/// // 1.5 USDC (6 decimals) = 1,500,000 atomic units
/// let amount = TokenAmount::new(U256::from(1_500_000u64), 6);
/// assert_eq!(amount.to_human_string(), "1.500000");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TokenAmount {
    /// The atomic amount (smallest unit, e.g., wei for ETH, or base units for tokens)
    atomic: U256,
    /// Number of decimal places for this token (e.g., 6 for USDC, 18 for ETH)
    decimals: u8,
}

impl TokenAmount {
    /// Create a new TokenAmount from atomic units and decimals.
    ///
    /// # Arguments
    ///
    /// * `atomic` - The amount in smallest units (e.g., wei, satoshi, or token base units)
    /// * `decimals` - The number of decimal places for formatting (e.g., 6 for USDC)
    pub const fn new(atomic: U256, decimals: u8) -> Self {
        Self { atomic, decimals }
    }

    /// Create a TokenAmount from a u128 value and decimals.
    pub fn from_u128(atomic: u128, decimals: u8) -> Self {
        Self {
            atomic: U256::from(atomic),
            decimals,
        }
    }

    /// Get the atomic (raw) amount as U256.
    pub const fn atomic(&self) -> U256 {
        self.atomic
    }

    /// Get the atomic amount as a string.
    pub fn atomic_string(&self) -> String {
        self.atomic.to_string()
    }

    /// Get the number of decimals for this token.
    pub const fn decimals(&self) -> u8 {
        self.decimals
    }

    /// Check if the amount is zero.
    pub fn is_zero(&self) -> bool {
        self.atomic == U256::ZERO
    }

    /// Convert to a human-readable string with proper decimal formatting.
    ///
    /// # Examples
    ///
    /// ```
    /// use purl::types::TokenAmount;
    /// use alloy::primitives::U256;
    ///
    /// let amount = TokenAmount::new(U256::from(1_500_000u64), 6);
    /// assert_eq!(amount.to_human_string(), "1.500000");
    ///
    /// let small = TokenAmount::new(U256::from(1u64), 6);
    /// assert_eq!(small.to_human_string(), "0.000001");
    /// ```
    pub fn to_human_string(&self) -> String {
        if self.decimals == 0 {
            return self.atomic.to_string();
        }

        let divisor = U256::from(10u64).pow(U256::from(self.decimals));
        let whole = self.atomic / divisor;
        let remainder = self.atomic % divisor;

        if remainder.is_zero() {
            format!("{}.{}", whole, "0".repeat(self.decimals as usize))
        } else {
            let remainder_str = format!("{:0>width$}", remainder, width = self.decimals as usize);
            format!("{}.{}", whole, remainder_str)
        }
    }

    /// Parse a human-readable amount string into a TokenAmount.
    ///
    /// # Arguments
    ///
    /// * `s` - A string like "1.5" or "100"
    /// * `decimals` - The number of decimals for this token
    ///
    /// # Errors
    ///
    /// Returns an error if the string cannot be parsed as a valid amount.
    pub fn parse_human(s: &str, decimals: u8) -> Result<Self, String> {
        let parts: Vec<&str> = s.split('.').collect();

        match parts.len() {
            1 => {
                // No decimal point, treat as whole number
                let whole: U256 = parts[0]
                    .parse()
                    .map_err(|_| format!("Invalid whole number: {}", parts[0]))?;
                let multiplier = U256::from(10u64).pow(U256::from(decimals));
                Ok(Self::new(whole * multiplier, decimals))
            }
            2 => {
                let whole: U256 = if parts[0].is_empty() {
                    U256::ZERO
                } else {
                    parts[0]
                        .parse()
                        .map_err(|_| format!("Invalid whole number: {}", parts[0]))?
                };

                let frac_str = parts[1];
                if frac_str.len() > decimals as usize {
                    return Err(format!(
                        "Too many decimal places: {} (max {})",
                        frac_str.len(),
                        decimals
                    ));
                }

                // Pad the fractional part to the right number of decimals
                let padded = format!("{:0<width$}", frac_str, width = decimals as usize);
                let frac: U256 = padded
                    .parse()
                    .map_err(|_| format!("Invalid fractional part: {}", frac_str))?;

                let multiplier = U256::from(10u64).pow(U256::from(decimals));
                Ok(Self::new(whole * multiplier + frac, decimals))
            }
            _ => Err(format!("Invalid amount format: {}", s)),
        }
    }
}

impl fmt::Display for TokenAmount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_human_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_amount_new() {
        let amount = TokenAmount::new(U256::from(1_000_000u64), 6);
        assert_eq!(amount.atomic(), U256::from(1_000_000u64));
        assert_eq!(amount.decimals(), 6);
    }

    #[test]
    fn test_token_amount_from_u128() {
        let amount = TokenAmount::from_u128(1_500_000, 6);
        assert_eq!(amount.atomic(), U256::from(1_500_000u64));
        assert_eq!(amount.decimals(), 6);
    }

    #[test]
    fn test_token_amount_atomic_string() {
        let amount = TokenAmount::new(U256::from(1_500_000u64), 6);
        assert_eq!(amount.atomic_string(), "1500000");
    }

    #[test]
    fn test_token_amount_is_zero() {
        let zero = TokenAmount::new(U256::ZERO, 6);
        assert!(zero.is_zero());

        let nonzero = TokenAmount::new(U256::from(1u64), 6);
        assert!(!nonzero.is_zero());
    }

    #[test]
    fn test_token_amount_to_human_string() {
        // 1.5 USDC
        let amount = TokenAmount::new(U256::from(1_500_000u64), 6);
        assert_eq!(amount.to_human_string(), "1.500000");

        // 0.000001 USDC (1 atomic unit)
        let small = TokenAmount::new(U256::from(1u64), 6);
        assert_eq!(small.to_human_string(), "0.000001");

        // 1000 USDC (whole number)
        let large = TokenAmount::new(U256::from(1_000_000_000u64), 6);
        assert_eq!(large.to_human_string(), "1000.000000");

        // Zero
        let zero = TokenAmount::new(U256::ZERO, 6);
        assert_eq!(zero.to_human_string(), "0.000000");
    }

    #[test]
    fn test_token_amount_zero_decimals() {
        let amount = TokenAmount::new(U256::from(42u64), 0);
        assert_eq!(amount.to_human_string(), "42");
    }

    #[test]
    fn test_token_amount_display() {
        let amount = TokenAmount::new(U256::from(1_500_000u64), 6);
        assert_eq!(format!("{}", amount), "1.500000");
    }

    #[test]
    fn test_parse_human_whole_number() {
        let amount = TokenAmount::parse_human("100", 6).expect("should parse");
        assert_eq!(amount.atomic(), U256::from(100_000_000u64));
        assert_eq!(amount.decimals(), 6);
    }

    #[test]
    fn test_parse_human_with_decimals() {
        let amount = TokenAmount::parse_human("1.5", 6).expect("should parse");
        assert_eq!(amount.atomic(), U256::from(1_500_000u64));
    }

    #[test]
    fn test_parse_human_small_amount() {
        let amount = TokenAmount::parse_human("0.000001", 6).expect("should parse");
        assert_eq!(amount.atomic(), U256::from(1u64));
    }

    #[test]
    fn test_parse_human_too_many_decimals() {
        let result = TokenAmount::parse_human("1.1234567", 6);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_human_invalid() {
        assert!(TokenAmount::parse_human("abc", 6).is_err());
        assert!(TokenAmount::parse_human("1.2.3", 6).is_err());
    }
}
