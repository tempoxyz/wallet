//! Value formatting helpers (amounts, durations, timestamps).

use crate::network::NetworkId;

/// Format atomic token units as a human-readable string with trimmed trailing zeros.
pub fn format_token_amount(atomic: u128, network: NetworkId) -> String {
    let t = network.token();
    let formatted =
        alloy::primitives::utils::format_units(atomic, t.decimals).expect("decimals <= 77");
    if let Some(stripped) = formatted.strip_suffix(&format!(".{}", "0".repeat(t.decimals as usize)))
    {
        format!("{stripped} {}", t.symbol)
    } else {
        let trimmed = formatted.trim_end_matches('0');
        format!("{trimmed} {}", t.symbol)
    }
}

/// Current UTC time as an ISO-8601 string (e.g. `2024-01-15T12:00:00Z`).
pub fn now_utc() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    format_utc_timestamp(now)
}

/// Format a Unix timestamp as an ISO-8601 UTC string (e.g. `2024-01-15T12:00:00Z`).
pub fn format_utc_timestamp(timestamp: u64) -> String {
    let secs = i64::try_from(timestamp).unwrap_or(i64::MAX);
    let dt =
        time::OffsetDateTime::from_unix_timestamp(secs).unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        dt.year(),
        dt.month() as u8,
        dt.day(),
        dt.hour(),
        dt.minute(),
        dt.second()
    )
}

/// Format seconds as a human-readable duration (e.g., "1h 30m", "2m 5s").
pub fn format_duration(secs: u64) -> String {
    if secs >= 86400 {
        let d = secs / 86400;
        let h = (secs % 86400) / 3600;
        if h == 0 {
            format!("{d}d")
        } else {
            format!("{d}d {h}h")
        }
    } else if secs >= 3600 {
        let h = secs / 3600;
        let m = (secs % 3600) / 60;
        if m == 0 {
            format!("{h}h")
        } else {
            format!("{h}h {m}m")
        }
    } else if secs >= 60 {
        let m = secs / 60;
        let s = secs % 60;
        if s == 0 {
            format!("{m}m")
        } else {
            format!("{m}m {s}s")
        }
    } else {
        format!("{secs}s")
    }
}

/// Format a Unix timestamp as a human-readable relative time (e.g., "5m ago", "2h ago", "3d ago").
pub fn format_relative_time(ts: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if ts == 0 || ts > now {
        return "just now".to_string();
    }
    let ago = now - ts;
    if ago < 60 {
        format!("{ago}s ago")
    } else if ago < 3600 {
        format!("{}m ago", ago / 60)
    } else if ago < 86400 {
        format!("{}h ago", ago / 3600)
    } else {
        format!("{}d ago", ago / 86400)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_token_amount() {
        assert_eq!(format_token_amount(1_000_000, NetworkId::Tempo), "1 USDC");
        assert_eq!(format_token_amount(1_500_000, NetworkId::Tempo), "1.5 USDC");
        assert_eq!(format_token_amount(123, NetworkId::Tempo), "0.000123 USDC");
        assert_eq!(format_token_amount(0, NetworkId::Tempo), "0 USDC");
    }

    #[test]
    fn test_format_duration_zero() {
        assert_eq!(format_duration(0), "0s");
    }

    #[test]
    fn test_format_duration_seconds() {
        assert_eq!(format_duration(1), "1s");
        assert_eq!(format_duration(59), "59s");
    }

    #[test]
    fn test_format_duration_exact_minutes() {
        assert_eq!(format_duration(60), "1m");
        assert_eq!(format_duration(120), "2m");
        assert_eq!(format_duration(900), "15m");
    }

    #[test]
    fn test_format_duration_minutes_and_seconds() {
        assert_eq!(format_duration(61), "1m 1s");
        assert_eq!(format_duration(90), "1m 30s");
        assert_eq!(format_duration(125), "2m 5s");
    }

    #[test]
    fn test_format_duration_hours() {
        assert_eq!(format_duration(3600), "1h");
        assert_eq!(format_duration(3661), "1h 1m");
        assert_eq!(format_duration(7200), "2h");
        assert_eq!(format_duration(5400), "1h 30m");
    }

    #[test]
    fn test_format_duration_days() {
        assert_eq!(format_duration(86400), "1d");
        assert_eq!(format_duration(90000), "1d 1h");
        assert_eq!(format_duration(172800), "2d");
    }
}
