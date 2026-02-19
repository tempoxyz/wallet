//! Currency definitions and utilities for token amounts

use alloy::primitives::U256;

#[cfg(test)]
pub use tests::{Money, TokenId};

/// Format a U256 value with the given number of decimal places.
///
/// This is the core formatting function that handles U256 directly,
/// avoiding any truncation to u128.
pub fn format_u256_with_decimals(value: U256, decimals: u8) -> String {
    if decimals == 0 {
        return value.to_string();
    }

    let divisor = U256::from(10u64).pow(U256::from(decimals));
    let whole = value / divisor;
    let remainder = value % divisor;

    // Format remainder with leading zeros
    let remainder_str = remainder.to_string();
    let padded = format!("{:0>width$}", remainder_str, width = decimals as usize);

    format!("{}.{}", whole, padded)
}

/// Represents a cryptocurrency or token with its metadata
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Currency {
    /// Symbol/ticker (e.g., "USDC", "pathUSD")
    pub symbol: &'static str,
    /// Full name (e.g., "USDC", "pathUSD")
    pub name: &'static str,
    /// Number of decimal places
    pub decimals: u8,
    /// Divisor for converting atomic units to human-readable (10^decimals)
    pub divisor: u64,
}

impl Currency {
    /// Create a new currency with calculated divisor
    pub const fn new(symbol: &'static str, name: &'static str, decimals: u8) -> Self {
        let divisor = 10u64.pow(decimals as u32);
        Self {
            symbol,
            name,
            decimals,
            divisor,
        }
    }

    /// Format atomic units to human-readable string with appropriate decimal places
    #[cfg(test)]
    pub fn format_atomic(&self, atomic: u128) -> String {
        let divisor = self.divisor as u128;
        let whole = atomic / divisor;
        let remainder = atomic % divisor;

        if self.decimals == 0 {
            whole.to_string()
        } else {
            let frac_str = format!("{:0width$}", remainder, width = self.decimals as usize);
            format!("{whole}.{frac_str}")
        }
    }

    /// Parse atomic units from a string
    #[cfg(test)]
    pub fn parse_atomic(&self, atomic_str: &str) -> Result<u128, std::num::ParseIntError> {
        atomic_str.parse()
    }

    /// Format atomic units with symbol
    #[cfg(test)]
    pub fn format_with_symbol(&self, atomic: u128) -> String {
        format!("{} {}", self.format_atomic(atomic), self.symbol)
    }

    /// Format atomic units with trimmed trailing zeros
    ///
    /// Unlike `format_atomic`, this trims trailing zeros for cleaner display.
    /// e.g., "1.500000 pathUSD" becomes "1.5 pathUSD"
    pub fn format_trimmed(&self, atomic: u128) -> String {
        let divisor = self.divisor as u128;
        let whole = atomic / divisor;
        let remainder = atomic % divisor;

        if remainder == 0 {
            format!("{whole} {}", self.symbol)
        } else {
            let frac_str = format!("{:0width$}", remainder, width = self.decimals as usize);
            let trimmed = frac_str.trim_end_matches('0');
            format!("{whole}.{trimmed} {}", self.symbol)
        }
    }

    /// Format atomic units from a string with trimmed trailing zeros
    ///
    /// Useful when the atomic value is provided as a string (e.g., from JSON).
    ///
    /// # Errors
    ///
    /// Returns an error if the string cannot be parsed as a valid u128.
    #[cfg(test)]
    pub fn format_trimmed_from_str(
        &self,
        atomic_str: &str,
    ) -> Result<String, std::num::ParseIntError> {
        let atomic: u128 = atomic_str.parse()?;
        Ok(self.format_trimmed(atomic))
    }
}

/// Tempo stablecoin definitions
pub mod currencies {
    use super::Currency;

    /// pathUSD - Tempo testnet stablecoin
    pub const PATH_USD: Currency = Currency::new("pathUSD", "pathUSD", 6);
    /// USDC - Bridged USDC on Tempo mainnet
    pub const USDCE: Currency = Currency::new("USDC", "USDC", 6);
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::error::{PrestoError, Result};
    use crate::network::Network;
    use alloy::primitives::Address;
    use std::fmt;
    use std::str::FromStr;

    // ==================== TokenId ====================

    /// Canonical identity for a token on a specific network.
    ///
    /// This prevents cross-chain and cross-token confusion by requiring
    /// both the network and asset address to match for operations.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct TokenId {
        network: Network,
        asset: Address,
    }

    impl TokenId {
        pub const fn new(network: Network, asset: Address) -> Self {
            Self { network, asset }
        }

        const fn network(&self) -> Network {
            self.network
        }

        fn from_network_and_address(network: Network, address: &str) -> Result<Self> {
            let asset = Address::from_str(address).map_err(|e| {
                PrestoError::invalid_address(format!("Invalid token address '{}': {}", address, e))
            })?;
            Ok(Self { network, asset })
        }

        fn default_for_network(network: Network) -> Option<Self> {
            let config = network.default_token_config();
            let asset = Address::from_str(config.address).ok()?;
            Some(Self { network, asset })
        }
    }

    impl fmt::Display for TokenId {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}:{:#x}", self.network, self.asset)
        }
    }

    // ==================== Money ====================

    /// A token amount with full type information.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Money {
        token: TokenId,
        atomic: U256,
        decimals: u8,
        symbol: String,
    }

    impl Money {
        pub fn new(token: TokenId, atomic: U256, decimals: u8, symbol: impl Into<String>) -> Self {
            Self {
                token,
                atomic,
                decimals,
                symbol: symbol.into(),
            }
        }

        fn from_network_config(network: Network, atomic: U256) -> Result<Self> {
            let config = network.default_token_config();
            let token = TokenId::from_network_and_address(network, config.address)?;
            Ok(Self {
                token,
                atomic,
                decimals: config.currency.decimals,
                symbol: config.currency.symbol.to_string(),
            })
        }

        fn from_atomic_str(
            token: TokenId,
            atomic_str: &str,
            decimals: u8,
            symbol: impl Into<String>,
        ) -> Result<Self> {
            let atomic = U256::from_str(atomic_str).map_err(|e| {
                PrestoError::InvalidAmount(format!("Invalid atomic amount '{}': {}", atomic_str, e))
            })?;
            Ok(Self::new(token, atomic, decimals, symbol))
        }

        fn from_human(
            human: &str,
            token: TokenId,
            decimals: u8,
            symbol: impl Into<String>,
        ) -> Result<Self> {
            let parts: Vec<&str> = human.split('.').collect();

            let atomic = match parts.len() {
                1 => {
                    let whole: U256 = parts[0].parse().map_err(|_| {
                        PrestoError::InvalidAmount(format!("Invalid number: {}", parts[0]))
                    })?;
                    let multiplier = U256::from(10u64).pow(U256::from(decimals));
                    whole * multiplier
                }
                2 => {
                    let whole: U256 = if parts[0].is_empty() {
                        U256::ZERO
                    } else {
                        parts[0].parse().map_err(|_| {
                            PrestoError::InvalidAmount(format!(
                                "Invalid whole number: {}",
                                parts[0]
                            ))
                        })?
                    };

                    let frac_str = parts[1];
                    if frac_str.len() > decimals as usize {
                        return Err(PrestoError::InvalidAmount(format!(
                            "Too many decimal places: {} (max {})",
                            frac_str.len(),
                            decimals
                        )));
                    }

                    let padded = format!("{:0<width$}", frac_str, width = decimals as usize);
                    let frac: U256 = padded.parse().map_err(|_| {
                        PrestoError::InvalidAmount(format!("Invalid fractional part: {}", frac_str))
                    })?;

                    let multiplier = U256::from(10u64).pow(U256::from(decimals));
                    whole * multiplier + frac
                }
                _ => {
                    return Err(PrestoError::InvalidAmount(format!(
                        "Invalid amount format: {}",
                        human
                    )));
                }
            };

            Ok(Self::new(token, atomic, decimals, symbol))
        }

        pub const fn network(&self) -> Network {
            self.token.network
        }

        pub const fn atomic(&self) -> U256 {
            self.atomic
        }

        const fn decimals(&self) -> u8 {
            self.decimals
        }

        fn symbol(&self) -> &str {
            &self.symbol
        }

        fn is_zero(&self) -> bool {
            self.atomic == U256::ZERO
        }

        fn format_human(&self) -> String {
            format_u256_with_decimals(self.atomic, self.decimals)
        }

        fn format_trimmed(&self) -> String {
            format_u256_trimmed(self.atomic, self.decimals, &self.symbol)
        }

        fn checked_add(&self, other: &Money) -> Result<Money> {
            if self.token != other.token {
                return Err(PrestoError::InvalidAmount(format!(
                    "Cannot add {} and {}: different tokens",
                    self.token, other.token
                )));
            }
            let result = self
                .atomic
                .checked_add(other.atomic)
                .ok_or_else(|| PrestoError::InvalidAmount("Overflow in addition".to_string()))?;
            Ok(Money {
                token: self.token,
                atomic: result,
                decimals: self.decimals,
                symbol: self.symbol.clone(),
            })
        }

        fn checked_sub(&self, other: &Money) -> Result<Money> {
            if self.token != other.token {
                return Err(PrestoError::InvalidAmount(format!(
                    "Cannot subtract {} and {}: different tokens",
                    self.token, other.token
                )));
            }
            let result = self.atomic.checked_sub(other.atomic).ok_or_else(|| {
                PrestoError::InvalidAmount("Underflow in subtraction".to_string())
            })?;
            Ok(Money {
                token: self.token,
                atomic: result,
                decimals: self.decimals,
                symbol: self.symbol.clone(),
            })
        }

        fn checked_cmp(&self, other: &Money) -> Result<std::cmp::Ordering> {
            if self.token != other.token {
                return Err(PrestoError::InvalidAmount(format!(
                    "Cannot compare {} and {}: different tokens",
                    self.token, other.token
                )));
            }
            Ok(self.atomic.cmp(&other.atomic))
        }
    }

    impl fmt::Display for Money {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "{}", self.format_trimmed())
        }
    }

    // ==================== Test Helpers ====================

    fn format_u256_trimmed(value: U256, decimals: u8, symbol: &str) -> String {
        if decimals == 0 {
            return format!("{} {}", value, symbol);
        }

        let divisor = U256::from(10u64).pow(U256::from(decimals));
        let whole = value / divisor;
        let remainder = value % divisor;

        if remainder == U256::ZERO {
            format!("{} {}", whole, symbol)
        } else {
            let remainder_str = remainder.to_string();
            let padded = format!("{:0>width$}", remainder_str, width = decimals as usize);
            let trimmed = padded.trim_end_matches('0');
            format!("{}.{} {}", whole, trimmed, symbol)
        }
    }

    fn test_token() -> TokenId {
        TokenId::new(
            Network::TempoModerato,
            Address::from_str("0x20c0000000000000000000000000000000000001")
                .expect("valid test address"),
        )
    }

    // ==================== Tests ====================

    #[test]
    fn test_path_usd_currency() {
        let path = currencies::PATH_USD;
        assert_eq!(path.symbol, "pathUSD");
        assert_eq!(path.name, "pathUSD");
        assert_eq!(path.decimals, 6);
        assert_eq!(path.divisor, 1_000_000);
    }

    #[test]
    fn test_all_tempo_currencies_have_6_decimals() {
        assert_eq!(currencies::PATH_USD.decimals, 6);
    }

    #[test]
    fn test_format_atomic() {
        let currency = currencies::PATH_USD;
        assert_eq!(currency.format_atomic(1_000_000), "1.000000");
        assert_eq!(currency.format_atomic(500_000), "0.500000");
        assert_eq!(currency.format_atomic(1), "0.000001");
        assert_eq!(currency.format_atomic(0), "0.000000");
        assert_eq!(currency.format_atomic(1_500_000), "1.500000");
    }

    #[test]
    fn test_format_with_symbol() {
        let currency = currencies::PATH_USD;
        assert_eq!(currency.format_with_symbol(1_000_000), "1.000000 pathUSD");
        assert_eq!(currency.format_with_symbol(500_000), "0.500000 pathUSD");
    }

    #[test]
    fn test_format_trimmed() {
        let path = currencies::PATH_USD;
        assert_eq!(path.format_trimmed(1_000_000), "1 pathUSD");
        assert_eq!(path.format_trimmed(1_500_000), "1.5 pathUSD");
        assert_eq!(path.format_trimmed(1_234_567), "1.234567 pathUSD");
        assert_eq!(path.format_trimmed(100_000), "0.1 pathUSD");
        assert_eq!(path.format_trimmed(0), "0 pathUSD");
    }

    #[test]
    fn test_format_trimmed_from_str() {
        let currency = currencies::PATH_USD;
        assert_eq!(
            currency.format_trimmed_from_str("1000000").unwrap(),
            "1 pathUSD"
        );
        assert_eq!(
            currency.format_trimmed_from_str("1500000").unwrap(),
            "1.5 pathUSD"
        );
        assert!(currency.format_trimmed_from_str("invalid").is_err());
    }

    #[test]
    fn test_parse_atomic() {
        let currency = currencies::PATH_USD;
        assert_eq!(
            currency
                .parse_atomic("1000000")
                .expect("Failed to parse 1000000"),
            1_000_000
        );
        assert_eq!(currency.parse_atomic("0").expect("Failed to parse 0"), 0);
        assert!(currency.parse_atomic("invalid").is_err());
    }

    #[test]
    fn test_currency_equality() {
        let path1 = currencies::PATH_USD;
        let path2 = Currency::new("pathUSD", "pathUSD", 6);
        assert_eq!(path1, path2);
    }

    #[test]
    fn test_divisor_calculation() {
        assert_eq!(currencies::PATH_USD.divisor, 1_000_000);
    }

    #[test]
    fn test_token_id_equality() {
        let token1 = test_token();
        let token2 = TokenId::new(
            Network::Tempo,
            Address::from_str("0x20c0000000000000000000000000000000000001")
                .expect("valid test address"),
        );

        let token1_copy = test_token();
        assert_eq!(token1, token1_copy);
        assert_ne!(token1, token2);
    }

    #[test]
    fn test_token_id_default_for_network() {
        let token = TokenId::default_for_network(Network::TempoModerato);
        assert!(token.is_some());
        assert_eq!(
            token
                .expect("TempoModerato should have token config")
                .network(),
            Network::TempoModerato
        );

        let token2 = TokenId::default_for_network(Network::Tempo);
        assert!(token2.is_some());
    }

    #[test]
    fn test_money_new() {
        let token = test_token();
        let money = Money::new(token, U256::from(1_500_000u64), 6, "αUSD");

        assert_eq!(money.atomic(), U256::from(1_500_000u64));
        assert_eq!(money.decimals(), 6);
        assert_eq!(money.symbol(), "αUSD");
        assert_eq!(money.network(), Network::TempoModerato);
    }

    #[test]
    fn test_money_from_network_config() {
        let money = Money::from_network_config(Network::TempoModerato, U256::from(1_000_000u64))
            .expect("TempoModerato has token config");

        assert_eq!(money.network(), Network::TempoModerato);
        assert_eq!(money.decimals(), 6);
        assert_eq!(money.symbol(), "pathUSD");
    }

    #[test]
    fn test_money_from_atomic_str() {
        let token = test_token();
        let money =
            Money::from_atomic_str(token, "1500000", 6, "pathUSD").expect("valid atomic string");

        assert_eq!(money.atomic(), U256::from(1_500_000u64));
    }

    #[test]
    fn test_money_from_human() {
        let token = test_token();

        let money = Money::from_human("100", token, 6, "pathUSD").expect("valid whole number");
        assert_eq!(money.atomic(), U256::from(100_000_000u64));

        let money = Money::from_human("1.5", token, 6, "pathUSD").expect("valid decimal");
        assert_eq!(money.atomic(), U256::from(1_500_000u64));

        let money = Money::from_human("0.000001", token, 6, "pathUSD").expect("valid small amount");
        assert_eq!(money.atomic(), U256::from(1u64));
    }

    #[test]
    fn test_money_from_human_errors() {
        let token = test_token();

        assert!(Money::from_human("1.1234567", token, 6, "pathUSD").is_err());
        assert!(Money::from_human("1.2.3", token, 6, "pathUSD").is_err());
        assert!(Money::from_human("abc", token, 6, "pathUSD").is_err());
    }

    #[test]
    fn test_format_human() {
        let token = test_token();

        let money = Money::new(token, U256::from(1_500_000u64), 6, "pathUSD");
        assert_eq!(money.format_human(), "1.500000");

        let money = Money::new(token, U256::from(1u64), 6, "pathUSD");
        assert_eq!(money.format_human(), "0.000001");

        let money = Money::new(token, U256::ZERO, 6, "pathUSD");
        assert_eq!(money.format_human(), "0.000000");
    }

    #[test]
    fn test_money_format_trimmed() {
        let token = test_token();

        let money = Money::new(token, U256::from(1_000_000u64), 6, "pathUSD");
        assert_eq!(money.format_trimmed(), "1 pathUSD");

        let money = Money::new(token, U256::from(1_500_000u64), 6, "pathUSD");
        assert_eq!(money.format_trimmed(), "1.5 pathUSD");

        let money = Money::new(token, U256::from(1_234_567u64), 6, "pathUSD");
        assert_eq!(money.format_trimmed(), "1.234567 pathUSD");
    }

    #[test]
    fn test_format_u256_large_values() {
        let large_value = U256::from(u128::MAX) + U256::from(1u64);
        let formatted = format_u256_with_decimals(large_value, 18);

        assert!(!formatted.is_empty());
        assert!(formatted.contains('.'));
    }

    #[test]
    fn test_checked_add() {
        let token = test_token();
        let money1 = Money::new(token, U256::from(1_000_000u64), 6, "pathUSD");
        let money2 = Money::new(token, U256::from(500_000u64), 6, "pathUSD");

        let result = money1.checked_add(&money2).expect("same token addition");
        assert_eq!(result.atomic(), U256::from(1_500_000u64));
    }

    #[test]
    fn test_checked_add_different_tokens() {
        let token1 = test_token();
        let token2 = TokenId::new(
            Network::Tempo,
            Address::from_str("0x20c0000000000000000000000000000000000001")
                .expect("valid test address"),
        );

        let money1 = Money::new(token1, U256::from(1_000_000u64), 6, "αUSD");
        let money2 = Money::new(token2, U256::from(500_000u64), 6, "αUSD");

        assert!(money1.checked_add(&money2).is_err());
    }

    #[test]
    fn test_checked_sub() {
        let token = test_token();
        let money1 = Money::new(token, U256::from(1_500_000u64), 6, "pathUSD");
        let money2 = Money::new(token, U256::from(500_000u64), 6, "pathUSD");

        let result = money1.checked_sub(&money2).expect("valid subtraction");
        assert_eq!(result.atomic(), U256::from(1_000_000u64));
    }

    #[test]
    fn test_checked_sub_underflow() {
        let token = test_token();
        let money1 = Money::new(token, U256::from(500_000u64), 6, "AlphaUSD");
        let money2 = Money::new(token, U256::from(1_000_000u64), 6, "AlphaUSD");

        assert!(money1.checked_sub(&money2).is_err());
    }

    #[test]
    fn test_checked_cmp() {
        let token = test_token();
        let money1 = Money::new(token, U256::from(1_500_000u64), 6, "AlphaUSD");
        let money2 = Money::new(token, U256::from(1_000_000u64), 6, "AlphaUSD");

        assert_eq!(
            money1.checked_cmp(&money2).expect("same token comparison"),
            std::cmp::Ordering::Greater
        );
    }

    #[test]
    fn test_is_zero() {
        let token = test_token();

        let zero = Money::new(token, U256::ZERO, 6, "AlphaUSD");
        assert!(zero.is_zero());

        let nonzero = Money::new(token, U256::from(1u64), 6, "AlphaUSD");
        assert!(!nonzero.is_zero());
    }

    #[test]
    fn test_display() {
        let token = test_token();
        let money = Money::new(token, U256::from(1_500_000u64), 6, "AlphaUSD");
        assert_eq!(format!("{}", money), "1.5 AlphaUSD");
    }

    #[test]
    fn test_token_id_display() {
        let token = test_token();
        let display = format!("{}", token);
        assert!(display.contains("tempo-moderato"));
        assert!(display.contains("0x"));
    }

    #[test]
    fn test_from_human_leading_decimal() {
        let token = test_token();
        let money = Money::from_human(".5", token, 6, "pathUSD").expect("leading decimal");
        assert_eq!(money.atomic(), U256::from(500_000u64));
    }

    #[test]
    fn test_from_human_trailing_decimal() {
        let token = test_token();
        let money = Money::from_human("1.", token, 6, "pathUSD").expect("trailing decimal");
        assert_eq!(money.atomic(), U256::from(1_000_000u64));
    }

    #[test]
    fn test_from_human_leading_zeros() {
        let token = test_token();
        let money = Money::from_human("0001.500", token, 6, "pathUSD").expect("leading zeros");
        assert_eq!(money.atomic(), U256::from(1_500_000u64));
    }
}
