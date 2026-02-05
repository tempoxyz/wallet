//! Shared utilities for CLI commands.

use std::time::{SystemTime, UNIX_EPOCH};

/// Get current Unix timestamp in seconds.
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Format an expiry timestamp as a human-readable string.
pub fn format_expiry(expiry: u64) -> String {
    if expiry == 0 {
        return "no expiry".to_string();
    }

    let now = now_secs();
    if expiry < now {
        "expired".to_string()
    } else {
        let remaining = expiry - now;
        let hours = remaining / 3600;
        let minutes = (remaining % 3600) / 60;
        format!("{}h {}m left", hours, minutes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_expiry_no_expiry() {
        assert_eq!(format_expiry(0), "no expiry");
    }

    #[test]
    fn test_format_expiry_expired() {
        let past = now_secs().saturating_sub(3600);
        assert_eq!(format_expiry(past), "expired");
    }

    #[test]
    fn test_format_expiry_future() {
        let future = now_secs() + 7200 + 1800;
        let result = format_expiry(future);
        assert!(result.contains("h") && result.contains("m left"));
    }

    #[test]
    fn test_now_secs_reasonable() {
        let now = now_secs();
        assert!(now > 1704067200);
        assert!(now < 4102444800);
    }
}
