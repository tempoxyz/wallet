//! Network types and explorer configuration.

pub mod explorer;
mod types;

pub use types::*;

use alloy::providers::RootProvider;
use alloy::rpc::client::RpcClient;
use alloy::transports::http::Http;
use base64::Engine;

/// Create an alloy HTTP provider that handles URL-embedded credentials.
///
/// `alloy`'s `ProviderBuilder::new().connect_http()` and `RootProvider::new_http()`
/// silently drop `user:pass@host` credentials from URLs. This helper extracts them
/// and injects a proper `Authorization: Basic` header via a custom reqwest client.
pub fn http_provider(rpc_url: &str) -> std::result::Result<RootProvider, String> {
    let parsed = reqwest::Url::parse(rpc_url).map_err(|e| format!("Invalid RPC URL: {e}"))?;

    if !parsed.username().is_empty() || parsed.password().is_some() {
        let username = parsed.username().to_string();
        let password = parsed.password().unwrap_or_default().to_string();
        let credentials = format!("{username}:{password}");
        let encoded = base64::engine::general_purpose::STANDARD.encode(credentials.as_bytes());
        let auth_value = format!("Basic {encoded}");

        let mut clean = parsed.clone();
        clean.set_username("").ok();
        clean.set_password(None).ok();

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_str(&auth_value)
                .map_err(|e| format!("Invalid auth header value: {e}"))?,
        );

        let client = reqwest::Client::builder()
            .default_headers(headers)
            .build()
            .map_err(|e| format!("Failed to build HTTP client: {e}"))?;

        let http = Http::with_client(client, clean);
        let rpc_client = RpcClient::new(http, false);
        Ok(RootProvider::new(rpc_client))
    } else {
        Ok(RootProvider::new_http(parsed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_provider_plain_url() {
        let provider = http_provider("https://rpc.example.com");
        assert!(provider.is_ok());
    }

    #[test]
    fn test_http_provider_with_credentials() {
        let provider = http_provider("https://user:pass@rpc.example.com");
        assert!(provider.is_ok());
    }

    #[test]
    fn test_http_provider_invalid_url() {
        let provider = http_provider("not a url");
        assert!(provider.is_err());
    }
}
