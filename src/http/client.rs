//! HTTP client implementation using reqwest.
//!
//! This module provides an async HTTP client for use with tokio.

use crate::error::Result;
use std::collections::HashMap;
use std::time::Duration;
use tracing::warn;

#[derive(Debug)]
pub struct HttpResponse {
    pub status_code: u32,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    /// Convert the response body to a UTF-8 string.
    ///
    /// # Errors
    /// Returns an error if the body is not valid UTF-8.
    pub fn body_string(&self) -> Result<String> {
        Ok(String::from_utf8(self.body.clone())?)
    }

    /// Check if this response indicates payment is required (HTTP 402).
    pub fn is_payment_required(&self) -> bool {
        self.status_code == 402
    }

    /// Get a header value by name (case-insensitive).
    pub fn get_header(&self, name: &str) -> Option<&String> {
        self.headers.get(&name.to_lowercase())
    }
}

/// Configuration for building HTTP clients.
///
/// This struct holds the configuration options that can be used to build
/// both async and blocking HTTP clients.
#[derive(Clone, Default)]
pub struct HttpClientConfig {
    pub(crate) verbose: bool,
    pub(crate) timeout: Option<u64>,
    pub(crate) follow_redirects: bool,
    pub(crate) user_agent: Option<String>,
    pub(crate) headers: Vec<(String, String)>,
}

/// Builder for configuring HTTP clients.
///
/// This provides a fluent API for setting up an HttpClient with various options.
#[must_use]
pub struct HttpClientBuilder {
    config: HttpClientConfig,
}

impl HttpClientBuilder {
    /// Create a new HTTP client builder with default settings.
    pub fn new() -> Self {
        Self {
            config: HttpClientConfig::default(),
        }
    }

    /// Enable verbose output for debugging.
    pub fn verbose(mut self, verbose: bool) -> Self {
        self.config.verbose = verbose;
        self
    }

    /// Set request timeout in seconds.
    pub fn timeout(mut self, seconds: u64) -> Self {
        self.config.timeout = Some(seconds);
        self
    }

    /// Enable following HTTP redirects.
    pub fn follow_redirects(mut self, follow: bool) -> Self {
        self.config.follow_redirects = follow;
        self
    }

    /// Set custom User-Agent header.
    pub fn user_agent(mut self, ua: impl Into<String>) -> Self {
        self.config.user_agent = Some(ua.into());
        self
    }

    /// Add multiple headers at once.
    pub fn headers(mut self, headers: &[(String, String)]) -> Self {
        self.config.headers.extend_from_slice(headers);
        self
    }

    /// Build the configured async HTTP client.
    pub fn build(self) -> Result<HttpClient> {
        HttpClient::from_config(self.config)
    }
}

impl Default for HttpClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Async HTTP client for making HTTP requests.
///
/// This is the primary HTTP client, using async/await for non-blocking I/O.
pub struct HttpClient {
    client: reqwest::Client,
}

impl HttpClient {
    /// Create a new async HTTP client with default settings.
    pub fn new() -> Result<Self> {
        HttpClientBuilder::new().build()
    }

    /// Create an async HTTP client from configuration.
    pub(crate) fn from_config(config: HttpClientConfig) -> Result<Self> {
        let mut builder = reqwest::Client::builder().connection_verbose(config.verbose);

        if let Some(timeout) = config.timeout {
            builder = builder.timeout(Duration::from_secs(timeout));
        }

        if config.follow_redirects {
            builder = builder.redirect(reqwest::redirect::Policy::limited(10));
        } else {
            builder = builder.redirect(reqwest::redirect::Policy::none());
        }

        if let Some(ref ua) = config.user_agent {
            builder = builder.user_agent(ua);
        }

        if !config.headers.is_empty() {
            let mut header_map = reqwest::header::HeaderMap::new();
            for (name, value) in &config.headers {
                let header_name = match reqwest::header::HeaderName::from_bytes(name.as_bytes()) {
                    Ok(n) => n,
                    Err(e) => {
                        warn!(header_name = %name, error = %e, "dropping header with invalid name");
                        continue;
                    }
                };
                let header_value = match reqwest::header::HeaderValue::from_str(value) {
                    Ok(v) => v,
                    Err(e) => {
                        warn!(header_name = %name, header_value = %value, error = %e, "dropping header with invalid value");
                        continue;
                    }
                };
                header_map.insert(header_name, header_value);
            }
            builder = builder.default_headers(header_map);
        }

        let client = builder.build()?;
        Ok(HttpClient { client })
    }

    /// Perform a request with the specified HTTP method and optional body.
    pub async fn request(
        &self,
        method: reqwest::Method,
        url: &str,
        body: Option<&[u8]>,
    ) -> Result<HttpResponse> {
        let mut request = self.client.request(method, url);

        if let Some(data) = body {
            request = request.body(data.to_vec());
        }

        let response = request.send().await?;
        Self::convert_response(response).await
    }

    /// Get a clone of the underlying reqwest client.
    ///
    /// Used for session/SSE flows that need direct access to reqwest's streaming API.
    pub fn inner_client(&self) -> reqwest::Client {
        self.client.clone()
    }

    /// Convert a reqwest response to our HttpResponse type
    async fn convert_response(response: reqwest::Response) -> Result<HttpResponse> {
        let status_code = response.status().as_u16() as u32;

        // Convert headers to HashMap with lowercase keys
        let mut headers = HashMap::new();
        for (key, value) in response.headers() {
            if let Ok(value_str) = value.to_str() {
                headers.insert(key.as_str().to_lowercase(), value_str.to_string());
            }
        }

        let body = response.bytes().await?.to_vec();

        Ok(HttpResponse {
            status_code,
            headers,
            body,
        })
    }
}

impl Default for HttpClient {
    fn default() -> Self {
        // HttpClient::new() internally uses HttpClientBuilder which only fails
        // if reqwest::Client::builder().build() fails. This is infallible in practice
        // since we're not doing any I/O or validation that could fail.
        // However, to be safe we provide a fallback that creates a basic client directly.
        Self::new().unwrap_or_else(|_| Self {
            client: reqwest::Client::new(),
        })
    }
}

/// Utility function to check if a header exists in the response (case-insensitive).
///
/// # Example
/// ```
/// use presto::http::has_header;
/// let headers = vec![
///     "Content-Type: application/json".to_string(),
///     "Content-Length: 123".to_string(),
/// ];
/// assert!(has_header(&headers, "content-type"));
/// assert!(has_header(&headers, "Content-Type"));
/// assert!(!has_header(&headers, "Authorization"));
/// ```
pub fn has_header(headers: &[String], name: &str) -> bool {
    let name_lower = name.to_lowercase();
    headers.iter().any(|h| {
        h.split_once(':')
            .is_some_and(|(k, _)| k.trim().to_lowercase() == name_lower)
    })
}

/// Parse raw header strings into a list of (name, value) pairs.
///
/// Preserves duplicate headers (important for HTTP headers like Set-Cookie).
/// Header names are lowercased for consistency. Malformed entries are skipped.
///
/// # Example
/// ```
/// use presto::http::parse_headers;
/// let headers = vec![
///     "Content-Type: application/json".to_string(),
///     "X-Custom: a".to_string(),
///     "X-Custom: b".to_string(),
/// ];
/// let parsed = parse_headers(&headers);
/// assert_eq!(parsed.len(), 3);
/// ```
pub fn parse_headers(headers: &[String]) -> Vec<(String, String)> {
    headers
        .iter()
        .filter_map(|header| {
            let (key, value) = header.split_once(':')?;
            Some((key.trim().to_lowercase(), value.trim().to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_has_header() {
        let headers = vec![
            "Content-Type: application/json".to_string(),
            "Content-Length: 123".to_string(),
        ];
        assert!(has_header(&headers, "content-type"));
        assert!(has_header(&headers, "Content-Type"));
        assert!(has_header(&headers, "CONTENT-TYPE"));
        assert!(!has_header(&headers, "Authorization"));
    }

    #[test]
    fn test_has_header_empty() {
        let headers: Vec<String> = vec![];
        assert!(!has_header(&headers, "Content-Type"));
    }

    #[test]
    fn test_has_header_malformed() {
        let headers = vec!["NoColonHeader".to_string()];
        assert!(!has_header(&headers, "NoColonHeader"));
    }

    #[test]
    fn test_has_header_with_whitespace() {
        let headers = vec!["  Content-Type  :  application/json  ".to_string()];
        assert!(has_header(&headers, "content-type"));
    }

    #[test]
    fn test_parse_headers() {
        let headers = vec![
            "Content-Type: application/json".to_string(),
            "Content-Length: 123".to_string(),
        ];
        let parsed = parse_headers(&headers);
        assert_eq!(parsed.len(), 2);
        assert_eq!(
            parsed[0],
            ("content-type".to_string(), "application/json".to_string())
        );
        assert_eq!(parsed[1], ("content-length".to_string(), "123".to_string()));
    }

    #[test]
    fn test_parse_headers_preserves_duplicates() {
        let headers = vec![
            "X-Custom: first".to_string(),
            "X-Custom: second".to_string(),
        ];
        let parsed = parse_headers(&headers);
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].1, "first");
        assert_eq!(parsed[1].1, "second");
    }

    #[test]
    fn test_parse_headers_skips_malformed() {
        let headers = vec![
            "Content-Type: application/json".to_string(),
            "MalformedHeader".to_string(),
            "Content-Length: 123".to_string(),
        ];
        let parsed = parse_headers(&headers);
        assert_eq!(parsed.len(), 2);
        assert!(parsed.iter().all(|(k, _)| k != "malformedheader"));
    }
}
