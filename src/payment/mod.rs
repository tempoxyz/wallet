//! Payment handling: charge (one-shot) and session (channel) flows.
//!
//! Routes HTTP 402 responses to the appropriate payment path,
//! builds and signs transactions, and retries with payment credentials.

pub(crate) mod charge;
pub(crate) mod dispatch;
pub(crate) mod session;

/// Extract the first meaningful error string from a JSON response body.
///
/// Checks `error`, `message`, and `detail` fields in order.
pub(crate) fn extract_json_error(body: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    json.get("error")
        .or_else(|| json.get("message"))
        .or_else(|| json.get("detail"))
        .and_then(|v| v.as_str())
        .map(String::from)
}
