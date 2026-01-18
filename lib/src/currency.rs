//! Currency definitions and utilities for token amounts

/// Represents a cryptocurrency or token with its metadata
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Currency {
    /// Symbol/ticker (e.g., "USDC", "ETH")
    pub symbol: &'static str,
    /// Full name (e.g., "USD Coin")
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
    pub fn format_atomic(&self, atomic: u128) -> String {
        let value = atomic as f64 / self.divisor as f64;
        format!("{:.1$}", value, self.decimals as usize)
    }

    /// Parse atomic units from a string
    pub fn parse_atomic(&self, atomic_str: &str) -> Result<u128, std::num::ParseIntError> {
        atomic_str.parse()
    }

    /// Format atomic units with symbol
    pub fn format_with_symbol(&self, atomic: u128) -> String {
        format!("{} {}", self.format_atomic(atomic), self.symbol)
    }

    /// Format atomic units with trimmed trailing zeros
    ///
    /// Unlike `format_atomic`, this trims trailing zeros for cleaner display.
    /// e.g., "1.500000 USDC" becomes "1.5 USDC"
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
    pub fn format_trimmed_from_str(&self, atomic_str: &str) -> String {
        let atomic: u128 = atomic_str.parse().unwrap_or(0);
        self.format_trimmed(atomic)
    }
}

/// Format atomic units to human-readable string with trimmed trailing zeros.
///
/// This is a standalone function for formatting when you don't have a Currency instance.
/// Uses the same logic as Currency::format_trimmed but accepts dynamic values.
pub fn format_atomic_trimmed(atomic_str: &str, decimals: u8, symbol: &str) -> String {
    let atomic: u128 = atomic_str.parse().unwrap_or(0);
    let divisor = 10_u128.pow(decimals as u32);
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

/// Parse a human-readable amount string with optional unit suffix.
///
/// Supports formats like:
/// - "1000000" - raw atomic units (no unit)
/// - "1.5usdc" or "1.5 USDC" - 1.5 USDC = 1500000 atomic units
/// - "0.01eth" or "0.01 ETH" - 0.01 ETH in wei
/// - "1.2sol" or "1.2 SOL" - 1.2 SOL in lamports
///
/// Returns the amount in atomic units.
pub fn parse_amount_with_unit(input: &str) -> Result<u128, AmountParseError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(AmountParseError::Empty);
    }

    // Try to split into number and unit
    let (num_part, unit_part) = split_number_and_unit(input);

    // If no unit, treat as raw atomic units
    if unit_part.is_empty() {
        return num_part
            .parse::<u128>()
            .map_err(|_| AmountParseError::InvalidNumber(num_part.to_string()));
    }

    // Look up the currency by symbol
    let currency = match unit_part.to_uppercase().as_str() {
        "USDC" => currencies::USDC,
        "ETH" | "ETHER" => currencies::ETH,
        "SOL" => currencies::SOL,
        "ALPHAUSD" => currencies::ALPHA_USD,
        "GWEI" => {
            // Special case: gwei is 10^9 wei
            let value: f64 = num_part
                .parse()
                .map_err(|_| AmountParseError::InvalidNumber(num_part.to_string()))?;
            let atomic = (value * 1_000_000_000.0).round() as u128;
            return Ok(atomic);
        }
        _ => return Err(AmountParseError::UnknownUnit(unit_part.to_string())),
    };

    // Parse the number and convert to atomic units
    let value: f64 = num_part
        .parse()
        .map_err(|_| AmountParseError::InvalidNumber(num_part.to_string()))?;
    let atomic = (value * currency.divisor as f64).round() as u128;

    Ok(atomic)
}

/// Split a string into numeric part and unit part
fn split_number_and_unit(input: &str) -> (&str, &str) {
    // Remove optional space between number and unit
    let input = input.trim();

    // Find where the number ends (consecutive digits, dots, signs)
    let num_end = input
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-' || *c == '+')
        .count();

    // Handle the byte position correctly for multi-byte chars
    let byte_pos = input
        .char_indices()
        .nth(num_end)
        .map(|(i, _)| i)
        .unwrap_or(input.len());

    let num_part = input[..byte_pos].trim();
    let unit_part = input[byte_pos..].trim();

    (num_part, unit_part)
}

/// Error type for amount parsing
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AmountParseError {
    Empty,
    InvalidNumber(String),
    UnknownUnit(String),
}

impl std::fmt::Display for AmountParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AmountParseError::Empty => write!(f, "empty amount string"),
            AmountParseError::InvalidNumber(s) => write!(f, "invalid number: '{s}'"),
            AmountParseError::UnknownUnit(s) => {
                write!(f, "unknown unit: '{s}' (supported: USDC, ETH, SOL, gwei)")
            }
        }
    }
}

impl std::error::Error for AmountParseError {}

/// Common currency definitions
pub mod currencies {
    use super::Currency;

    /// USD Coin (USDC) - 6 decimals
    pub const USDC: Currency = Currency::new("USDC", "USD Coin", 6);

    /// Ethereum (ETH) - 18 decimals
    pub const ETH: Currency = Currency::new("ETH", "Ethereum", 18);

    /// Solana (SOL) - 9 decimals
    pub const SOL: Currency = Currency::new("SOL", "Solana", 9);

    /// AlphaUSD (Tempo testnet stablecoin) - 6 decimals
    pub const ALPHA_USD: Currency = Currency::new("AlphaUSD", "AlphaUSD", 6);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_usdc_currency() {
        let usdc = currencies::USDC;
        assert_eq!(usdc.symbol, "USDC");
        assert_eq!(usdc.name, "USD Coin");
        assert_eq!(usdc.decimals, 6);
        assert_eq!(usdc.divisor, 1_000_000);
    }

    #[test]
    fn test_format_atomic_usdc() {
        let usdc = currencies::USDC;
        assert_eq!(usdc.format_atomic(1_000_000), "1.000000");
        assert_eq!(usdc.format_atomic(500_000), "0.500000");
        assert_eq!(usdc.format_atomic(1), "0.000001");
        assert_eq!(usdc.format_atomic(0), "0.000000");
        assert_eq!(usdc.format_atomic(1_500_000), "1.500000");
    }

    #[test]
    fn test_format_atomic_eth() {
        let eth = currencies::ETH;
        assert_eq!(
            eth.format_atomic(1_000_000_000_000_000_000),
            "1.000000000000000000"
        );
        assert_eq!(
            eth.format_atomic(500_000_000_000_000_000),
            "0.500000000000000000"
        );
    }

    #[test]
    fn test_format_atomic_sol() {
        let sol = currencies::SOL;
        assert_eq!(sol.format_atomic(1_000_000_000), "1.000000000");
        assert_eq!(sol.format_atomic(500_000_000), "0.500000000");
    }

    #[test]
    fn test_format_with_symbol() {
        let usdc = currencies::USDC;
        assert_eq!(usdc.format_with_symbol(1_000_000), "1.000000 USDC");
        assert_eq!(usdc.format_with_symbol(500_000), "0.500000 USDC");
    }

    #[test]
    fn test_format_trimmed() {
        let usdc = currencies::USDC;
        assert_eq!(usdc.format_trimmed(1_000_000), "1 USDC");
        assert_eq!(usdc.format_trimmed(1_500_000), "1.5 USDC");
        assert_eq!(usdc.format_trimmed(1_234_567), "1.234567 USDC");
        assert_eq!(usdc.format_trimmed(100_000), "0.1 USDC");
        assert_eq!(usdc.format_trimmed(0), "0 USDC");

        let eth = currencies::ETH;
        assert_eq!(eth.format_trimmed(1_000_000_000_000_000_000), "1 ETH");
        assert_eq!(eth.format_trimmed(100_000_000_000_000_000), "0.1 ETH");
    }

    #[test]
    fn test_format_trimmed_from_str() {
        let usdc = currencies::USDC;
        assert_eq!(usdc.format_trimmed_from_str("1000000"), "1 USDC");
        assert_eq!(usdc.format_trimmed_from_str("1500000"), "1.5 USDC");
        assert_eq!(usdc.format_trimmed_from_str("invalid"), "0 USDC");
    }

    #[test]
    fn test_parse_atomic() {
        let usdc = currencies::USDC;
        assert_eq!(usdc.parse_atomic("1000000").unwrap(), 1_000_000);
        assert_eq!(usdc.parse_atomic("0").unwrap(), 0);
        assert!(usdc.parse_atomic("invalid").is_err());
    }

    #[test]
    fn test_currency_equality() {
        let usdc1 = currencies::USDC;
        let usdc2 = Currency::new("USDC", "USD Coin", 6);
        assert_eq!(usdc1, usdc2);
    }

    #[test]
    fn test_divisor_calculation() {
        assert_eq!(currencies::USDC.divisor, 1_000_000);
        assert_eq!(currencies::ETH.divisor, 1_000_000_000_000_000_000);
        assert_eq!(currencies::SOL.divisor, 1_000_000_000);
    }

    #[test]
    fn test_parse_amount_with_unit_usdc() {
        // USDC has 6 decimals
        assert_eq!(parse_amount_with_unit("1usdc").unwrap(), 1_000_000);
        assert_eq!(parse_amount_with_unit("1 USDC").unwrap(), 1_000_000);
        assert_eq!(parse_amount_with_unit("1.5usdc").unwrap(), 1_500_000);
        assert_eq!(parse_amount_with_unit("0.01usdc").unwrap(), 10_000);
        assert_eq!(parse_amount_with_unit("100 USDC").unwrap(), 100_000_000);
    }

    #[test]
    fn test_parse_amount_with_unit_eth() {
        // ETH has 18 decimals
        assert_eq!(
            parse_amount_with_unit("1eth").unwrap(),
            1_000_000_000_000_000_000
        );
        assert_eq!(
            parse_amount_with_unit("0.01eth").unwrap(),
            10_000_000_000_000_000
        );
        assert_eq!(
            parse_amount_with_unit("1 ETH").unwrap(),
            1_000_000_000_000_000_000
        );
    }

    #[test]
    fn test_parse_amount_with_unit_gwei() {
        // gwei is 10^9 wei
        assert_eq!(parse_amount_with_unit("1gwei").unwrap(), 1_000_000_000);
        assert_eq!(parse_amount_with_unit("10 gwei").unwrap(), 10_000_000_000);
        assert_eq!(parse_amount_with_unit("0.5gwei").unwrap(), 500_000_000);
    }

    #[test]
    fn test_parse_amount_with_unit_raw() {
        // No unit = raw atomic units
        assert_eq!(parse_amount_with_unit("1000000").unwrap(), 1_000_000);
        assert_eq!(parse_amount_with_unit("0").unwrap(), 0);
        assert_eq!(
            parse_amount_with_unit("1000000000000000000").unwrap(),
            1_000_000_000_000_000_000
        );
    }

    #[test]
    fn test_parse_amount_with_unit_errors() {
        assert!(matches!(
            parse_amount_with_unit(""),
            Err(AmountParseError::Empty)
        ));
        // "abc" has no numeric prefix, so empty num_part leads to InvalidNumber
        // when trying to parse it as a number with "abc" as unit
        assert!(matches!(
            parse_amount_with_unit("abc"),
            Err(AmountParseError::UnknownUnit(_)) | Err(AmountParseError::InvalidNumber(_))
        ));
        assert!(matches!(
            parse_amount_with_unit("1xyz"),
            Err(AmountParseError::UnknownUnit(_))
        ));
        // Invalid number format
        assert!(matches!(
            parse_amount_with_unit("1.2.3usdc"),
            Err(AmountParseError::InvalidNumber(_))
        ));
    }
}
