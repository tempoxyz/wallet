//! Sanitization helpers for safe logging and diagnostics.

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
        format!("{}…", &err[..MAX_LEN])
    }
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
    fn sanitize_error_prevents_secret_leakage_in_long_body() {
        let msg = format!("server error: {}secret_api_key_12345", "a]".repeat(100));
        let result = sanitize_error(&msg);
        assert!(!result.contains("secret_api_key_12345"));
    }
}
