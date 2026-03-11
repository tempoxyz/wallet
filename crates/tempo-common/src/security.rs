//! Security utilities: safe logging, sanitization, redaction.

/// Sensitive header names whose values must be redacted in logs and diagnostics.
const SENSITIVE_HEADERS: &[&str] = &[
    "authorization",
    "proxy-authorization",
    "cookie",
    "set-cookie",
    "x-api-key",
];

/// Redact a header value for safe logging.
///
/// For sensitive headers (Authorization, Cookie, etc.) the credential portion
/// is replaced with `[REDACTED]`. For `Authorization` / `Proxy-Authorization`
/// the scheme (e.g. `Bearer`, `Basic`) is preserved so the log remains useful.
pub fn redact_header_value(name: &str, value: &str) -> String {
    let lower = name.to_lowercase();
    if !SENSITIVE_HEADERS.contains(&lower.as_str()) {
        return value.to_string();
    }

    if lower == "authorization" || lower == "proxy-authorization" {
        if let Some((scheme, _)) = value.split_once(' ') {
            return format!("{scheme} [REDACTED]");
        }
    }

    "[REDACTED]".to_string()
}

/// Strip query parameters and fragments from a URL for safe logging.
///
/// Query strings often contain secrets (`?api_key=...`, `?token=...`), so we
/// only keep the scheme + host + path.
pub fn redact_url(raw: &str) -> String {
    match url::Url::parse(raw) {
        Ok(mut parsed) => {
            if !parsed.username().is_empty() {
                let _ = parsed.set_username("[REDACTED]");
            }
            if parsed.password().is_some() {
                let _ = parsed.set_password(Some("[REDACTED]"));
            }
            parsed.set_query(None);
            parsed.set_fragment(None);
            parsed.to_string()
        }
        Err(_) => raw.to_string(),
    }
}

/// Truncate an error message to avoid leaking sensitive server responses.
pub fn sanitize_error(err: &str) -> String {
    const MAX_LEN: usize = 200;
    if err.len() <= MAX_LEN {
        err.to_string()
    } else {
        // Find the last valid UTF-8 char boundary at or before MAX_LEN
        let end = err
            .char_indices()
            .map(|(i, _)| i)
            .take_while(|&i| i <= MAX_LEN)
            .last()
            .unwrap_or(0);
        format!("{}…", &err[..end])
    }
}

/// Validate a `0x`-prefixed hex string (address or channel ID).
///
/// Rejects characters that agents commonly hallucinate: `?`, `#`, `%`,
/// whitespace, and any non-hex-digit after the prefix.
pub fn validate_hex_input(value: &str, label: &str) -> Result<(), crate::error::InputError> {
    if !value.starts_with("0x") {
        return Err(crate::error::InputError::InvalidHexInput(format!(
            "{label} must start with '0x'"
        )));
    }
    let hex_part = &value[2..];
    if hex_part.is_empty() {
        return Err(crate::error::InputError::InvalidHexInput(format!(
            "{label} is empty after '0x' prefix"
        )));
    }
    for (i, ch) in hex_part.char_indices() {
        if !ch.is_ascii_hexdigit() {
            let hint = match ch {
                '?' | '#' | '%' => format!(
                    "unexpected '{ch}' in {label} at position {pos} (possible hallucinated URL parameter)",
                    pos = i + 2
                ),
                _ if ch.is_whitespace() => {
                    format!("unexpected whitespace in {label} at position {pos}", pos = i + 2)
                }
                _ => format!("invalid character '{ch}' in {label} at position {pos}", pos = i + 2),
            };
            return Err(crate::error::InputError::InvalidHexInput(hint));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redact_bearer_token() {
        assert_eq!(
            redact_header_value("Authorization", "Bearer sk_live_abc123"),
            "Bearer [REDACTED]"
        );
    }

    #[test]
    fn test_redact_basic_auth() {
        assert_eq!(
            redact_header_value("authorization", "Basic dXNlcjpwYXNz"),
            "Basic [REDACTED]"
        );
    }

    #[test]
    fn test_redact_proxy_authorization() {
        assert_eq!(
            redact_header_value("Proxy-Authorization", "Bearer proxy_token"),
            "Bearer [REDACTED]"
        );
    }

    #[test]
    fn test_redact_cookie() {
        assert_eq!(
            redact_header_value("cookie", "session=abc123; token=xyz"),
            "[REDACTED]"
        );
    }

    #[test]
    fn test_redact_set_cookie() {
        assert_eq!(
            redact_header_value("Set-Cookie", "sid=secret; Path=/; HttpOnly"),
            "[REDACTED]"
        );
    }

    #[test]
    fn test_redact_x_api_key() {
        assert_eq!(
            redact_header_value("X-Api-Key", "[REDACTED:sk-secret]"),
            "[REDACTED]"
        );
    }

    #[test]
    fn test_redact_auth_no_scheme() {
        assert_eq!(
            redact_header_value("Authorization", "tokenonly"),
            "[REDACTED]"
        );
    }

    #[test]
    fn test_redact_safe_header_unchanged() {
        assert_eq!(
            redact_header_value("Content-Type", "application/json"),
            "application/json"
        );
    }

    #[test]
    fn test_redact_accept_unchanged() {
        assert_eq!(redact_header_value("accept", "text/html"), "text/html");
    }

    #[test]
    fn test_redact_url_strips_secrets() {
        assert_eq!(
            redact_url("https://api.example.com/v1?api_key=secret"),
            "https://api.example.com/v1"
        );
    }

    #[test]
    fn test_redact_url_preserves_path() {
        assert_eq!(
            redact_url("https://api.example.com/v1/chat"),
            "https://api.example.com/v1/chat"
        );
    }

    #[test]
    fn test_redact_url_strips_basic_auth() {
        assert_eq!(
            redact_url("https://alice:s3cr3t@api.example.com/v1?token=abc"),
            "https://%5BREDACTED%5D:%5BREDACTED%5D@api.example.com/v1"
        );
    }

    #[test]
    fn test_redact_url_strips_username_only() {
        assert_eq!(
            redact_url("https://user@api.example.com/path"),
            "https://%5BREDACTED%5D@api.example.com/path"
        );
    }

    #[test]
    fn sanitize_error_short_unchanged() {
        let short = "connection refused";
        assert_eq!(sanitize_error(short), short);
    }

    #[test]
    fn sanitize_error_exactly_200_unchanged() {
        let msg = "x".repeat(200);
        assert_eq!(sanitize_error(&msg), msg);
    }

    #[test]
    fn sanitize_error_truncates_long_message() {
        let msg = "x".repeat(300);
        let result = sanitize_error(&msg);
        assert_eq!(result.len(), 200 + "…".len());
        assert!(result.ends_with('…'));
        assert!(result.starts_with("xxx"));
    }

    #[test]
    fn sanitize_error_multibyte_no_panic() {
        // 101 × 2-byte chars = 202 bytes, boundary falls mid-char without the fix
        let msg = "é".repeat(101);
        assert!(msg.len() > 200);
        let result = sanitize_error(&msg);
        assert!(result.ends_with('…'));
        // Must not panic and must be valid UTF-8 (implicit by being a String)
    }

    #[test]
    fn sanitize_error_prevents_secret_leakage_in_long_body() {
        let msg = format!("server error: {}secret_api_key_12345", "a]".repeat(100));
        let result = sanitize_error(&msg);
        assert!(!result.contains("secret_api_key_12345"));
    }

    #[test]
    fn validate_hex_input_valid_address() {
        assert!(
            validate_hex_input("0xabcdef1234567890abcdef1234567890abcdef12", "address").is_ok()
        );
    }

    #[test]
    fn validate_hex_input_valid_channel_id() {
        assert!(validate_hex_input(
            "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
            "channel ID"
        )
        .is_ok());
    }

    #[test]
    fn validate_hex_input_rejects_question_mark() {
        let result = validate_hex_input("0xabc?def", "address");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("hallucinated"));
    }

    #[test]
    fn validate_hex_input_rejects_hash() {
        assert!(validate_hex_input("0xabc#def", "address").is_err());
    }

    #[test]
    fn validate_hex_input_rejects_percent() {
        assert!(validate_hex_input("0xabc%20def", "address").is_err());
    }

    #[test]
    fn validate_hex_input_rejects_whitespace() {
        assert!(validate_hex_input("0xabc def", "address").is_err());
    }

    #[test]
    fn validate_hex_input_rejects_no_prefix() {
        assert!(validate_hex_input("abcdef", "address").is_err());
    }

    #[test]
    fn validate_hex_input_rejects_empty_hex() {
        assert!(validate_hex_input("0x", "address").is_err());
    }
}
