//! Analytics event types and payload definitions.

use serde::Serialize;

/// Strip query parameters and fragments from a URL before sending to analytics.
///
/// Query strings often contain secrets (`?api_key=...`, `?token=...`), so we
/// only keep the scheme + host + path.
pub fn sanitize_url(raw: &str) -> String {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    LoginStarted,
    LoginSuccess,
    LoginFailure,
    LoginTimeout,
    Logout,

    QueryStarted,
    QuerySuccess,
    QueryFailure,

    PaymentStarted,
    PaymentSuccess,
    PaymentFailure,

    KeyCreated,
    WhoamiViewed,

    CallbackWindowOpened,
    CallbackReceived,

    SessionStarted,
    CommandRun,
}

impl Event {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::LoginStarted => "login_started",
            Self::LoginSuccess => "login_success",
            Self::LoginFailure => "login_failure",
            Self::LoginTimeout => "login_timeout",
            Self::Logout => "logout",
            Self::QueryStarted => "query_started",
            Self::QuerySuccess => "query_success",
            Self::QueryFailure => "query_failure",
            Self::PaymentStarted => "payment_started",
            Self::PaymentSuccess => "payment_success",
            Self::PaymentFailure => "payment_failure",

            Self::KeyCreated => "key_created",
            Self::WhoamiViewed => "whoami_viewed",
            Self::CallbackWindowOpened => "callback_window_opened",
            Self::CallbackReceived => "callback_received",
            Self::SessionStarted => "session_started",
            Self::CommandRun => "command_run",
        }
    }
}

/// Trait for analytics event payloads.
///
/// The `'static` bound is required because payloads are moved into `tokio::spawn` tasks.
pub trait EventPayload: Serialize + Send + Sync + 'static {}

#[derive(Debug, Clone, Serialize)]
pub struct EmptyPayload;
impl EventPayload for EmptyPayload {}

#[derive(Debug, Clone, Serialize)]
pub struct LoginPayload {
    pub network: String,
}
impl EventPayload for LoginPayload {}

#[derive(Debug, Clone, Serialize)]
pub struct LoginFailurePayload {
    pub network: String,
    pub error: String,
}
impl EventPayload for LoginFailurePayload {}

#[derive(Debug, Clone, Serialize)]
pub struct QueryStartedPayload {
    pub url: String,
    pub method: String,
}
impl EventPayload for QueryStartedPayload {}

#[derive(Debug, Clone, Serialize)]
pub struct QuerySuccessPayload {
    pub url: String,
    pub method: String,
    pub status_code: u16,
}
impl EventPayload for QuerySuccessPayload {}

#[derive(Debug, Clone, Serialize)]
pub struct QueryFailurePayload {
    pub url: String,
    pub method: String,
    pub error: String,
}
impl EventPayload for QueryFailurePayload {}

#[derive(Debug, Clone, Serialize)]
pub struct PaymentStartedPayload {
    pub network: String,
    pub amount: String,
    pub currency: String,
}
impl EventPayload for PaymentStartedPayload {}

#[derive(Debug, Clone, Serialize)]
pub struct PaymentSuccessPayload {
    pub network: String,
    pub amount: String,
    pub currency: String,
    pub tx_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}
impl EventPayload for PaymentSuccessPayload {}

#[derive(Debug, Clone, Serialize)]
pub struct PaymentFailurePayload {
    pub network: String,
    pub amount: String,
    pub currency: String,
    pub error: String,
}
impl EventPayload for PaymentFailurePayload {}

#[derive(Debug, Clone, Serialize)]
pub struct CommandRunPayload {
    pub command: String,
}
impl EventPayload for CommandRunPayload {}

#[derive(Debug, Clone, Serialize)]
pub struct SessionStartedPayload {
    pub is_new_user: bool,
    pub command: String,
}
impl EventPayload for SessionStartedPayload {}

#[derive(Debug, Clone, Serialize)]
pub struct KeyCreatedPayload {
    pub network: String,
}
impl EventPayload for KeyCreatedPayload {}

#[derive(Debug, Clone, Serialize)]
pub struct CallbackWindowOpenedPayload {
    pub network: String,
}
impl EventPayload for CallbackWindowOpenedPayload {}

#[derive(Debug, Clone, Serialize)]
pub struct CallbackReceivedPayload {
    pub network: String,
    pub duration_secs: u64,
}
impl EventPayload for CallbackReceivedPayload {}

#[derive(Debug, Clone, Serialize)]
pub struct LoginTimeoutPayload {
    pub network: String,
}
impl EventPayload for LoginTimeoutPayload {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_url_strips_query_params() {
        assert_eq!(
            sanitize_url("https://api.example.com/v1?api_key=secret123&token=abc"),
            "https://api.example.com/v1"
        );
    }

    #[test]
    fn sanitize_url_strips_fragment() {
        assert_eq!(
            sanitize_url("https://api.example.com/v1#section"),
            "https://api.example.com/v1"
        );
    }

    #[test]
    fn sanitize_url_strips_both_query_and_fragment() {
        assert_eq!(
            sanitize_url("https://api.example.com/path?key=val#frag"),
            "https://api.example.com/path"
        );
    }

    #[test]
    fn sanitize_url_preserves_path() {
        assert_eq!(
            sanitize_url("https://api.example.com/v1/chat/completions"),
            "https://api.example.com/v1/chat/completions"
        );
    }

    #[test]
    fn sanitize_url_preserves_port() {
        assert_eq!(
            sanitize_url("http://127.0.0.1:8080/api?token=secret"),
            "http://127.0.0.1:8080/api"
        );
    }

    #[test]
    fn sanitize_url_invalid_url_passthrough() {
        assert_eq!(sanitize_url("not-a-url"), "not-a-url");
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
        // Simulate a server response that might contain a secret deep in the body
        let msg = format!("server error: {}secret_api_key_12345", "a]".repeat(100));
        let result = sanitize_error(&msg);
        assert!(!result.contains("secret_api_key_12345"));
    }
}
