//! Type-safe token amount handling with [`TokenId`] and [`Money`].
#![allow(dead_code)]

use crate::error::{PgetError, Result};
use crate::network::Network;
use alloy::primitives::{Address, U256};
use std::fmt;
use std::str::FromStr;

/// Canonical identity for a token on a specific network.
///
/// This prevents cross-chain and cross-token confusion by requiring
/// both the network and asset address to match for operations.
///
/// # Examples
///
/// ```
/// use pget::payment::money::TokenId;
/// use pget::network::{Network, tempo_tokens};
/// use alloy::primitives::Address;
/// use std::str::FromStr;
///
/// let path_usd = TokenId::new(
///     Network::Tempo,
///     Address::from_str(tempo_tokens::PATH_USD).unwrap(),
/// );
///
/// let alpha_usd = TokenId::new(
///     Network::Tempo,
///     Address::from_str(tempo_tokens::ALPHA_USD).unwrap(),
/// );
///
/// // Different tokens on same network
/// assert_ne!(path_usd, alpha_usd);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TokenId {
    /// The network this token exists on
    network: Network,
    /// The token contract address
    asset: Address,
}

impl TokenId {
    /// Create a new token identity.
    pub const fn new(network: Network, asset: Address) -> Self {
        Self { network, asset }
    }

    /// Get the network for this token.
    pub const fn network(&self) -> Network {
        self.network
    }

    /// Get the asset address for this token.
    pub const fn asset(&self) -> Address {
        self.asset
    }

    /// Create a TokenId from network and address string.
    ///
    /// # Errors
    ///
    /// Returns an error if the address string is not a valid EVM address.
    pub fn from_network_and_address(network: Network, address: &str) -> Result<Self> {
        let asset = Address::from_str(address).map_err(|e| {
            PgetError::invalid_address(format!("Invalid token address '{}': {}", address, e))
        })?;
        Ok(Self { network, asset })
    }

    /// Get the default token for this network (pathUSD).
    ///
    /// Returns None if the network doesn't have a configured token.
    pub fn default_for_network(network: Network) -> Option<Self> {
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

/// A token amount with full type information.
///
/// Money is the single source of truth for representing token amounts in pget.
/// It combines the token identity, atomic amount, decimals, and symbol to
/// provide type-safe operations and formatting.
///
/// # Design
///
/// - Uses U256 internally to prevent truncation (never u128)
/// - Includes TokenId to prevent cross-token operations
/// - Centralizes all formatting logic
/// - Provides checked arithmetic operations
///
/// # Examples
///
/// ```
/// use pget::payment::money::{Money, TokenId};
/// use pget::network::{Network, tempo_tokens};
/// use alloy::primitives::{Address, U256};
/// use std::str::FromStr;
///
/// // Create 1.5 pathUSD on Tempo
/// let token = TokenId::new(
///     Network::Tempo,
///     Address::from_str(tempo_tokens::PATH_USD).unwrap(),
/// );
/// let amount = Money::new(token, U256::from(1_500_000u64), 6, "pathUSD");
///
/// assert_eq!(amount.format_human(), "1.500000");
/// assert_eq!(amount.format_trimmed(), "1.5 pathUSD");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Money {
    /// The token identity (network + asset)
    token: TokenId,
    /// The atomic amount (smallest unit)
    atomic: U256,
    /// Number of decimal places
    decimals: u8,
    /// Token symbol for display
    symbol: String,
}

impl Money {
    /// Create a new Money instance.
    ///
    /// # Arguments
    ///
    /// * `token` - The token identity (network + asset address)
    /// * `atomic` - The amount in atomic units (e.g., wei, base units)
    /// * `decimals` - Number of decimal places for human formatting
    /// * `symbol` - Token symbol for display (e.g., "pathUSD", "AlphaUSD")
    pub fn new(token: TokenId, atomic: U256, decimals: u8, symbol: impl Into<String>) -> Self {
        Self {
            token,
            atomic,
            decimals,
            symbol: symbol.into(),
        }
    }

    /// Create Money from a network's default token configuration (pathUSD).
    ///
    /// This is the recommended way to create Money for balance queries
    /// and payment operations when no specific token is specified.
    pub fn from_network_config(network: Network, atomic: U256) -> Result<Self> {
        let config = network.default_token_config();

        let token = TokenId::from_network_and_address(network, config.address)?;

        Ok(Self {
            token,
            atomic,
            decimals: config.currency.decimals,
            symbol: config.currency.symbol.to_string(),
        })
    }

    /// Create Money by parsing an atomic amount string.
    ///
    /// # Errors
    ///
    /// Returns an error if the string cannot be parsed as U256.
    pub fn from_atomic_str(
        token: TokenId,
        atomic_str: &str,
        decimals: u8,
        symbol: impl Into<String>,
    ) -> Result<Self> {
        let atomic = U256::from_str(atomic_str).map_err(|e| {
            PgetError::InvalidAmount(format!("Invalid atomic amount '{}': {}", atomic_str, e))
        })?;
        Ok(Self::new(token, atomic, decimals, symbol))
    }

    /// Parse a human-readable amount string into Money.
    ///
    /// # Arguments
    ///
    /// * `human` - A string like "1.5" or "100"
    /// * `token` - The token identity
    /// * `decimals` - Number of decimal places
    /// * `symbol` - Token symbol
    ///
    /// # Errors
    ///
    /// Returns an error if the string cannot be parsed.
    pub fn from_human(
        human: &str,
        token: TokenId,
        decimals: u8,
        symbol: impl Into<String>,
    ) -> Result<Self> {
        let parts: Vec<&str> = human.split('.').collect();

        let atomic = match parts.len() {
            1 => {
                // No decimal point, treat as whole number
                let whole: U256 = parts[0].parse().map_err(|_| {
                    PgetError::InvalidAmount(format!("Invalid number: {}", parts[0]))
                })?;
                let multiplier = U256::from(10u64).pow(U256::from(decimals));
                whole * multiplier
            }
            2 => {
                let whole: U256 = if parts[0].is_empty() {
                    U256::ZERO
                } else {
                    parts[0].parse().map_err(|_| {
                        PgetError::InvalidAmount(format!("Invalid whole number: {}", parts[0]))
                    })?
                };

                let frac_str = parts[1];
                if frac_str.len() > decimals as usize {
                    return Err(PgetError::InvalidAmount(format!(
                        "Too many decimal places: {} (max {})",
                        frac_str.len(),
                        decimals
                    )));
                }

                // Pad the fractional part to the right number of decimals
                let padded = format!("{:0<width$}", frac_str, width = decimals as usize);
                let frac: U256 = padded.parse().map_err(|_| {
                    PgetError::InvalidAmount(format!("Invalid fractional part: {}", frac_str))
                })?;

                let multiplier = U256::from(10u64).pow(U256::from(decimals));
                whole * multiplier + frac
            }
            _ => {
                return Err(PgetError::InvalidAmount(format!(
                    "Invalid amount format: {}",
                    human
                )));
            }
        };

        Ok(Self::new(token, atomic, decimals, symbol))
    }

    // ==================== Accessors ====================

    /// Get the token identity.
    pub const fn token(&self) -> &TokenId {
        &self.token
    }

    /// Get the network this money is on.
    pub const fn network(&self) -> Network {
        self.token.network
    }

    /// Get the asset address.
    pub const fn asset(&self) -> Address {
        self.token.asset
    }

    /// Get the atomic amount as U256.
    pub const fn atomic(&self) -> U256 {
        self.atomic
    }

    /// Get the atomic amount as a string.
    pub fn atomic_string(&self) -> String {
        self.atomic.to_string()
    }

    /// Get the number of decimals.
    pub const fn decimals(&self) -> u8 {
        self.decimals
    }

    /// Get the token symbol.
    pub fn symbol(&self) -> &str {
        &self.symbol
    }

    /// Check if the amount is zero.
    pub fn is_zero(&self) -> bool {
        self.atomic == U256::ZERO
    }

    // ==================== Formatting ====================

    /// Format the amount as a human-readable string with full decimal places.
    ///
    /// This always includes all decimal places (e.g., "1.500000" for 6 decimals).
    pub fn format_human(&self) -> String {
        format_u256_with_decimals(self.atomic, self.decimals)
    }

    /// Format the amount with symbol and full decimal places.
    pub fn format_with_symbol(&self) -> String {
        format!("{} {}", self.format_human(), self.symbol)
    }

    /// Format the amount with trimmed trailing zeros.
    ///
    /// More compact display: "1.5 pathUSD" instead of "1.500000 pathUSD"
    pub fn format_trimmed(&self) -> String {
        format_u256_trimmed(self.atomic, self.decimals, &self.symbol)
    }

    /// Format just the amount with trimmed zeros (no symbol).
    pub fn format_trimmed_amount(&self) -> String {
        let formatted = format_u256_with_decimals(self.atomic, self.decimals);
        trim_trailing_zeros(&formatted)
    }

    // ==================== Checked Arithmetic ====================

    /// Add two Money values, verifying they are the same token.
    ///
    /// # Errors
    ///
    /// Returns an error if the tokens don't match or if overflow occurs.
    pub fn checked_add(&self, other: &Money) -> Result<Money> {
        if self.token != other.token {
            return Err(PgetError::InvalidAmount(format!(
                "Cannot add {} and {}: different tokens",
                self.token, other.token
            )));
        }

        let result = self
            .atomic
            .checked_add(other.atomic)
            .ok_or_else(|| PgetError::InvalidAmount("Overflow in addition".to_string()))?;

        Ok(Money {
            token: self.token,
            atomic: result,
            decimals: self.decimals,
            symbol: self.symbol.clone(),
        })
    }

    /// Subtract two Money values, verifying they are the same token.
    ///
    /// # Errors
    ///
    /// Returns an error if the tokens don't match or if underflow occurs.
    pub fn checked_sub(&self, other: &Money) -> Result<Money> {
        if self.token != other.token {
            return Err(PgetError::InvalidAmount(format!(
                "Cannot subtract {} and {}: different tokens",
                self.token, other.token
            )));
        }

        let result = self
            .atomic
            .checked_sub(other.atomic)
            .ok_or_else(|| PgetError::InvalidAmount("Underflow in subtraction".to_string()))?;

        Ok(Money {
            token: self.token,
            atomic: result,
            decimals: self.decimals,
            symbol: self.symbol.clone(),
        })
    }

    /// Compare two Money values, verifying they are the same token.
    ///
    /// # Errors
    ///
    /// Returns an error if the tokens don't match.
    pub fn checked_cmp(&self, other: &Money) -> Result<std::cmp::Ordering> {
        if self.token != other.token {
            return Err(PgetError::InvalidAmount(format!(
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

// ==================== Formatting Helpers ====================

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

/// Format a U256 value with trimmed trailing zeros and symbol.
pub fn format_u256_trimmed(value: U256, decimals: u8, symbol: &str) -> String {
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

/// Trim trailing zeros from a decimal string.
fn trim_trailing_zeros(s: &str) -> String {
    if let Some(dot_pos) = s.find('.') {
        let (whole, frac) = s.split_at(dot_pos);
        let trimmed_frac = frac[1..].trim_end_matches('0');
        if trimmed_frac.is_empty() {
            whole.to_string()
        } else {
            format!("{}.{}", whole, trimmed_frac)
        }
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_token() -> TokenId {
        TokenId::new(
            Network::TempoModerato,
            Address::from_str("0x20c0000000000000000000000000000000000001")
                .expect("valid test address"),
        )
    }

    #[test]
    fn test_token_id_equality() {
        let token1 = test_token();
        let token2 = TokenId::new(
            Network::Tempo,
            Address::from_str("0x20c0000000000000000000000000000000000001")
                .expect("valid test address"),
        );

        // Same network and address = equal
        let token1_copy = test_token();
        assert_eq!(token1, token1_copy);

        // Different network = not equal
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
        assert_eq!(money.symbol(), "pathUSD"); // default token is pathUSD
    }

    #[test]
    fn test_money_from_atomic_str() {
        let token = test_token();
        let money =
            Money::from_atomic_str(token, "1500000", 6, "AlphaUSD").expect("valid atomic string");

        assert_eq!(money.atomic(), U256::from(1_500_000u64));
    }

    #[test]
    fn test_money_from_human() {
        let token = test_token();

        // Whole number
        let money = Money::from_human("100", token, 6, "AlphaUSD").expect("valid whole number");
        assert_eq!(money.atomic(), U256::from(100_000_000u64));

        // With decimals
        let money = Money::from_human("1.5", token, 6, "AlphaUSD").expect("valid decimal");
        assert_eq!(money.atomic(), U256::from(1_500_000u64));

        // Small amount
        let money =
            Money::from_human("0.000001", token, 6, "AlphaUSD").expect("valid small amount");
        assert_eq!(money.atomic(), U256::from(1u64));
    }

    #[test]
    fn test_money_from_human_errors() {
        let token = test_token();

        // Too many decimals
        assert!(Money::from_human("1.1234567", token, 6, "AlphaUSD").is_err());

        // Invalid format
        assert!(Money::from_human("1.2.3", token, 6, "AlphaUSD").is_err());

        // Invalid number
        assert!(Money::from_human("abc", token, 6, "AlphaUSD").is_err());
    }

    #[test]
    fn test_format_human() {
        let token = test_token();

        let money = Money::new(token, U256::from(1_500_000u64), 6, "AlphaUSD");
        assert_eq!(money.format_human(), "1.500000");

        let money = Money::new(token, U256::from(1u64), 6, "AlphaUSD");
        assert_eq!(money.format_human(), "0.000001");

        let money = Money::new(token, U256::ZERO, 6, "AlphaUSD");
        assert_eq!(money.format_human(), "0.000000");
    }

    #[test]
    fn test_format_trimmed() {
        let token = test_token();

        let money = Money::new(token, U256::from(1_000_000u64), 6, "AlphaUSD");
        assert_eq!(money.format_trimmed(), "1 AlphaUSD");

        let money = Money::new(token, U256::from(1_500_000u64), 6, "AlphaUSD");
        assert_eq!(money.format_trimmed(), "1.5 AlphaUSD");

        let money = Money::new(token, U256::from(1_234_567u64), 6, "AlphaUSD");
        assert_eq!(money.format_trimmed(), "1.234567 AlphaUSD");
    }

    #[test]
    fn test_format_u256_large_values() {
        // Test with values larger than u128::MAX
        let large_value = U256::from(u128::MAX) + U256::from(1u64);
        let formatted = format_u256_with_decimals(large_value, 18);

        // Should not panic and should produce valid output
        assert!(!formatted.is_empty());
        assert!(formatted.contains('.'));
    }

    #[test]
    fn test_checked_add() {
        let token = test_token();
        let money1 = Money::new(token, U256::from(1_000_000u64), 6, "AlphaUSD");
        let money2 = Money::new(token, U256::from(500_000u64), 6, "AlphaUSD");

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

        // Should fail because tokens are different (different networks)
        assert!(money1.checked_add(&money2).is_err());
    }

    #[test]
    fn test_checked_sub() {
        let token = test_token();
        let money1 = Money::new(token, U256::from(1_500_000u64), 6, "AlphaUSD");
        let money2 = Money::new(token, U256::from(500_000u64), 6, "AlphaUSD");

        let result = money1.checked_sub(&money2).expect("valid subtraction");
        assert_eq!(result.atomic(), U256::from(1_000_000u64));
    }

    #[test]
    fn test_checked_sub_underflow() {
        let token = test_token();
        let money1 = Money::new(token, U256::from(500_000u64), 6, "AlphaUSD");
        let money2 = Money::new(token, U256::from(1_000_000u64), 6, "AlphaUSD");

        // Should fail due to underflow
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
}
