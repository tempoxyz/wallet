#![forbid(unsafe_code)]
#![deny(warnings)]
//! presto — a command-line HTTP client with automatic payment support.
//!
//! Presto works like curl/wget but handles HTTP 402 (Payment Required)
//! responses automatically using the [Machine Payments Protocol (MPP)](https://mpp.dev).
//!
//! # Payment flow
//!
//! 1. Send the initial HTTP request
//! 2. If the server responds with 402, parse the `WWW-Authenticate` header
//! 3. Construct and submit a payment via the user's configured wallet
//! 4. Retry the request with a payment credential
//!
//! # Payment intents
//!
//! - **Charge** — one-shot payment settled on-chain per request
//! - **Session** — opens a payment channel on-chain, then exchanges
//!   off-chain vouchers for each subsequent request or SSE token,
//!   settling when the session is closed
//!
//! # Security
//!
//! - Server-controlled text is sanitized before terminal output to
//!   prevent ANSI escape injection (OSC 8 breakout, cursor manipulation)
//! - Redirect targets are validated against an allow-list to prevent
//!   payment credential leakage to unintended hosts
//! - Private keys are stored in the OS keychain (macOS Keychain) or
//!   in a mode-0600 file, and wrapped in [`zeroize::Zeroizing`] in memory

mod account;
mod analytics;
mod cli;
mod config;
mod error;
mod http;
mod keys;
mod network;
mod payment;
mod util;
mod version;

use crate::cli::exit_codes::ExitCode;
use crate::cli::{Cli, OutputFormat};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let output_format = cli.resolve_output_format();
    let result = cli.run().await;

    if let Err(e) = result {
        match output_format {
            OutputFormat::Json | OutputFormat::Toon => {
                let output = render_error(&e, output_format);
                println!("{output}");
            }
            _ => {
                eprintln!("Error: {e:#}");
            }
        }
        ExitCode::from(&e).exit();
    }
}

/// Render a structured error object for agent consumption.
///
/// Schema: `{ code, message, cause? }`
fn render_error(err: &anyhow::Error, format: OutputFormat) -> String {
    let code = ExitCode::from(err).label();
    let message = err.to_string();
    let cause = err.chain().nth(1).map(|c| c.to_string());

    let mut obj = serde_json::json!({
        "code": code,
        "message": message,
    });

    if let Some(c) = cause {
        if let serde_json::Value::Object(ref mut map) = obj {
            map.insert("cause".into(), serde_json::Value::String(c));
        }
    }

    format.serialize(&obj).unwrap_or_else(|_| {
        format!("{{\"code\":\"{code}\",\"message\":\"error\"}}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::PrestoError;

    #[test]
    fn test_render_error_json_payment_rejected() {
        let err: anyhow::Error = PrestoError::PaymentRejected {
            reason: "insufficient funds".into(),
            status_code: 402,
        }
        .into();
        let json_str = render_error(&err, OutputFormat::Json);
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed["code"], "E_PAYMENT");
        assert!(parsed["message"]
            .as_str()
            .unwrap()
            .contains("insufficient funds"));
    }

    #[test]
    fn test_render_error_json_config_missing() {
        let err: anyhow::Error = PrestoError::ConfigMissing("no wallet".into()).into();
        let json_str = render_error(&err, OutputFormat::Json);
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed["code"], "E_USAGE");
    }

    #[test]
    fn test_render_error_json_has_cause() {
        let inner: anyhow::Error = PrestoError::Http("connection refused".into()).into();
        let err = inner.context("failed to reach server");
        let json_str = render_error(&err, OutputFormat::Json);
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert!(parsed["message"]
            .as_str()
            .unwrap()
            .contains("failed to reach server"));
        assert!(parsed["cause"]
            .as_str()
            .unwrap()
            .contains("connection refused"));
    }

    #[test]
    fn test_render_error_json_no_cause() {
        let err: anyhow::Error = PrestoError::InvalidUrl("bad scheme".into()).into();
        let json_str = render_error(&err, OutputFormat::Json);
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert!(parsed.get("cause").is_none());
    }

    #[test]
    fn test_render_error_json_schema_fields() {
        let err: anyhow::Error = PrestoError::UnknownNetwork("custom".into()).into();
        let json_str = render_error(&err, OutputFormat::Json);
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        let obj = parsed.as_object().unwrap();
        assert!(obj.contains_key("code"));
        assert!(obj.contains_key("message"));
        for key in obj.keys() {
            assert!(
                key == "code" || key == "message" || key == "cause",
                "unexpected field: {key}"
            );
        }
    }

    #[test]
    fn test_render_error_toon_roundtrip() {
        let err: anyhow::Error = PrestoError::Http("timeout".into()).into();
        let toon_str = render_error(&err, OutputFormat::Toon);
        let parsed: serde_json::Value = toon_format::decode_default(&toon_str).unwrap();
        assert_eq!(parsed["code"], "E_NETWORK");
        assert!(parsed["message"].as_str().unwrap().contains("timeout"));
    }
}
