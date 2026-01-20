//! HTTP client implementation using reqwest.
//!
//! This module provides an async-first HTTP client with an optional blocking wrapper.
//! The default API is asynchronous, suitable for use with async runtimes.
//!
//! For synchronous code, enable the `blocking` feature and use `http::blocking::HttpClient`.

use crate::error::Result;
use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;
use std::time::Duration;
use tracing::warn;

/// HTTP request methods.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum HttpMethod {
    #[default]
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
    /// Custom HTTP method (e.g., "CONNECT", "TRACE", or non-standard methods)
    Custom(String),
}

impl HttpMethod {
    /// Returns the method as an uppercase string.
    pub fn as_str(&self) -> &str {
        match self {
            HttpMethod::Get => "GET",
            HttpMethod::Post => "POST",
            HttpMethod::Put => "PUT",
            HttpMethod::Patch => "PATCH",
            HttpMethod::Delete => "DELETE",
            HttpMethod::Head => "HEAD",
            HttpMethod::Options => "OPTIONS",
            HttpMethod::Custom(s) => s,
        }
    }

    /// Returns true if this method typically has a request body.
    pub fn has_body(&self) -> bool {
        matches!(
            self,
            HttpMethod::Post | HttpMethod::Put | HttpMethod::Patch | HttpMethod::Custom(_)
        )
    }
}

impl fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for HttpMethod {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(match s.to_uppercase().as_str() {
            "GET" => HttpMethod::Get,
            "POST" => HttpMethod::Post,
            "PUT" => HttpMethod::Put,
            "PATCH" => HttpMethod::Patch,
            "DELETE" => HttpMethod::Delete,
            "HEAD" => HttpMethod::Head,
            "OPTIONS" => HttpMethod::Options,
            _ => HttpMethod::Custom(s.to_uppercase()),
        })
    }
}

// Note: We don't implement From<&str> to avoid unwrap() in conversion.
// Use .parse() instead, which returns Result (though the error type is Infallible).
// This makes the API more explicit and idiomatic.

impl From<&String> for HttpMethod {
    fn from(s: &String) -> Self {
        // Safe because FromStr::Err is Infallible
        s.parse().expect("HttpMethod::from_str cannot fail")
    }
}

impl From<String> for HttpMethod {
    fn from(s: String) -> Self {
        // Safe because FromStr::Err is Infallible
        s.parse().expect("HttpMethod::from_str cannot fail")
    }
}

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

    /// Add a custom HTTP header.
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.config.headers.push((name.into(), value.into()));
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

    /// Build a blocking HTTP client (requires `blocking` feature).
    #[cfg(feature = "blocking")]
    pub fn build_blocking(self) -> Result<blocking::HttpClient> {
        blocking::HttpClient::from_config(self.config)
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
/// For synchronous code, use `http::blocking::HttpClient` instead (requires `blocking` feature).
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

    /// Perform a GET request
    pub async fn get(&self, url: &str) -> Result<HttpResponse> {
        let response = self.client.get(url).send().await?;
        Self::convert_response(response).await
    }

    /// Perform a POST request with optional body
    pub async fn post(&self, url: &str, body: Option<&[u8]>) -> Result<HttpResponse> {
        let mut request = self.client.post(url);

        if let Some(data) = body {
            request = request.body(data.to_vec());
        }

        let response = request.send().await?;
        Self::convert_response(response).await
    }

    /// Perform a request with the specified HTTP method and optional body.
    ///
    /// This method accepts any type that implements `Into<HttpMethod>`, including
    /// `&str` for convenience.
    pub async fn request(
        &self,
        method: impl Into<HttpMethod>,
        url: &str,
        body: Option<&[u8]>,
    ) -> Result<HttpResponse> {
        let method = method.into();

        let reqwest_method = match &method {
            HttpMethod::Get => reqwest::Method::GET,
            HttpMethod::Post => reqwest::Method::POST,
            HttpMethod::Put => reqwest::Method::PUT,
            HttpMethod::Patch => reqwest::Method::PATCH,
            HttpMethod::Delete => reqwest::Method::DELETE,
            HttpMethod::Head => reqwest::Method::HEAD,
            HttpMethod::Options => reqwest::Method::OPTIONS,
            HttpMethod::Custom(s) => reqwest::Method::from_bytes(s.as_bytes())
                .map_err(|e| crate::error::PurlError::UnsupportedHttpMethod(e.to_string()))?,
        };

        let mut request = self.client.request(reqwest_method, url);

        if let Some(data) = body {
            request = request.body(data.to_vec());
        }

        let response = request.send().await?;
        Self::convert_response(response).await
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

/// Blocking HTTP client module (requires `blocking` feature).
///
/// This module provides a synchronous HTTP client API for use in non-async contexts.
#[cfg(feature = "blocking")]
pub mod blocking {
    use super::*;

    /// Blocking HTTP client for synchronous HTTP requests.
    ///
    /// This provides the same API as the async `HttpClient`, but with synchronous methods.
    /// Use this in non-async contexts or when you need blocking I/O.
    pub struct HttpClient {
        client: reqwest::blocking::Client,
    }

    impl HttpClient {
        /// Create a new blocking HTTP client with default settings.
        pub fn new() -> Result<Self> {
            HttpClientBuilder::new().build_blocking()
        }

        /// Create a blocking HTTP client from configuration.
        pub(crate) fn from_config(config: HttpClientConfig) -> Result<Self> {
            let mut builder =
                reqwest::blocking::Client::builder().connection_verbose(config.verbose);

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
                    let header_name = match reqwest::header::HeaderName::from_bytes(name.as_bytes())
                    {
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

        /// Perform a GET request
        pub fn get(&self, url: &str) -> Result<HttpResponse> {
            let response = self.client.get(url).send()?;
            Self::convert_response(response)
        }

        /// Perform a POST request with optional body
        pub fn post(&self, url: &str, body: Option<&[u8]>) -> Result<HttpResponse> {
            let mut request = self.client.post(url);

            if let Some(data) = body {
                request = request.body(data.to_vec());
            }

            let response = request.send()?;
            Self::convert_response(response)
        }

        /// Perform a request with the specified HTTP method and optional body.
        ///
        /// This method accepts any type that implements `Into<HttpMethod>`, including
        /// `&str` for convenience.
        pub fn request(
            &self,
            method: impl Into<HttpMethod>,
            url: &str,
            body: Option<&[u8]>,
        ) -> Result<HttpResponse> {
            let method = method.into();

            let reqwest_method = match &method {
                HttpMethod::Get => reqwest::Method::GET,
                HttpMethod::Post => reqwest::Method::POST,
                HttpMethod::Put => reqwest::Method::PUT,
                HttpMethod::Patch => reqwest::Method::PATCH,
                HttpMethod::Delete => reqwest::Method::DELETE,
                HttpMethod::Head => reqwest::Method::HEAD,
                HttpMethod::Options => reqwest::Method::OPTIONS,
                HttpMethod::Custom(s) => reqwest::Method::from_bytes(s.as_bytes())
                    .map_err(|e| crate::error::PurlError::UnsupportedHttpMethod(e.to_string()))?,
            };

            let mut request = self.client.request(reqwest_method, url);

            if let Some(data) = body {
                request = request.body(data.to_vec());
            }

            let response = request.send()?;
            Self::convert_response(response)
        }

        /// Convert a reqwest response to our HttpResponse type
        fn convert_response(response: reqwest::blocking::Response) -> Result<HttpResponse> {
            let status_code = response.status().as_u16() as u32;

            // Convert headers to HashMap with lowercase keys
            let mut headers = HashMap::new();
            for (key, value) in response.headers() {
                if let Ok(value_str) = value.to_str() {
                    headers.insert(key.as_str().to_lowercase(), value_str.to_string());
                }
            }

            let body = response.bytes()?.to_vec();

            Ok(HttpResponse {
                status_code,
                headers,
                body,
            })
        }
    }

    impl Default for HttpClient {
        fn default() -> Self {
            Self::new().unwrap_or_else(|_| Self {
                client: reqwest::blocking::Client::new(),
            })
        }
    }
}

/// Utility function to check if a header exists in the response (case-insensitive).
///
/// # Example
/// ```
/// use purl::http::has_header;
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

/// Find a header value by name (case-insensitive).
///
/// Returns the header value if found, None otherwise.
///
/// # Example
/// ```
/// use purl::http::find_header;
/// let headers = vec![
///     "Content-Type: application/json".to_string(),
///     "Content-Length: 123".to_string(),
/// ];
/// assert_eq!(find_header(&headers, "content-type"), Some("application/json".to_string()));
/// assert_eq!(find_header(&headers, "Content-Type"), Some("application/json".to_string()));
/// assert_eq!(find_header(&headers, "Authorization"), None);
/// ```
pub fn find_header(headers: &[String], name: &str) -> Option<String> {
    let name_lower = name.to_lowercase();
    headers.iter().find_map(|h| {
        let (key, value) = h.split_once(':')?;

        if key.trim().to_lowercase() == name_lower {
            Some(value.trim().to_string())
        } else {
            None
        }
    })
}

/// Parse raw header strings into a HashMap.
///
/// Converts headers from "Name: Value" format into a case-insensitive map.
///
/// # Example
/// ```
/// use purl::http::parse_headers;
/// let headers = vec![
///     "Content-Type: application/json".to_string(),
///     "Content-Length: 123".to_string(),
/// ];
/// let map = parse_headers(&headers);
/// assert_eq!(map.get("content-type"), Some(&"application/json".to_string()));
/// assert_eq!(map.get("content-length"), Some(&"123".to_string()));
/// ```
pub fn parse_headers(headers: &[String]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for header in headers {
        if let Some((key, value)) = header.split_once(':') {
            map.insert(key.trim().to_lowercase(), value.trim().to_string());
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_method_as_str() {
        assert_eq!(HttpMethod::Get.as_str(), "GET");
        assert_eq!(HttpMethod::Post.as_str(), "POST");
        assert_eq!(HttpMethod::Custom("TRACE".to_string()).as_str(), "TRACE");
    }

    #[test]
    fn test_http_method_from_str() {
        assert_eq!(
            "GET".parse::<HttpMethod>().expect("Failed to parse GET"),
            HttpMethod::Get
        );
        assert_eq!(
            "post".parse::<HttpMethod>().expect("Failed to parse post"),
            HttpMethod::Post
        );
        assert_eq!(
            "TRACE"
                .parse::<HttpMethod>()
                .expect("Failed to parse TRACE"),
            HttpMethod::Custom("TRACE".to_string())
        );
    }

    #[test]
    fn test_http_method_from_str_case_insensitive() {
        assert_eq!(
            "get".parse::<HttpMethod>().expect("Failed to parse get"),
            HttpMethod::Get
        );
        assert_eq!(
            "Post".parse::<HttpMethod>().expect("Failed to parse Post"),
            HttpMethod::Post
        );
        assert_eq!(
            "PUT".parse::<HttpMethod>().expect("Failed to parse PUT"),
            HttpMethod::Put
        );
    }

    #[test]
    fn test_http_method_display() {
        assert_eq!(format!("{}", HttpMethod::Get), "GET");
        assert_eq!(format!("{}", HttpMethod::Post), "POST");
    }

    #[test]
    fn test_http_method_equality() {
        assert_eq!(HttpMethod::Get, HttpMethod::Get);
        assert_ne!(HttpMethod::Get, HttpMethod::Post);
        assert_eq!(
            HttpMethod::Custom("TRACE".to_string()),
            HttpMethod::Custom("TRACE".to_string())
        );
    }

    #[test]
    fn test_http_method_clone() {
        let method = HttpMethod::Get;
        let cloned = method.clone();
        assert_eq!(method, cloned);
    }

    #[test]
    fn test_http_method_default() {
        assert_eq!(HttpMethod::default(), HttpMethod::Get);
    }

    #[test]
    fn test_http_method_has_body() {
        assert!(!HttpMethod::Get.has_body());
        assert!(HttpMethod::Post.has_body());
        assert!(HttpMethod::Put.has_body());
        assert!(HttpMethod::Patch.has_body());
        assert!(!HttpMethod::Delete.has_body());
        assert!(!HttpMethod::Head.has_body());
        assert!(HttpMethod::Custom("FOOBAR".to_string()).has_body());
    }

    #[test]
    fn test_http_method_hash() {
        use std::collections::HashMap;
        let mut map = HashMap::new();
        map.insert(HttpMethod::Get, "value");
        assert_eq!(map.get(&HttpMethod::Get), Some(&"value"));
    }

    #[test]
    fn test_http_method_parse() {
        let method: HttpMethod = "GET".parse().expect("Failed to parse GET");
        assert_eq!(method, HttpMethod::Get);
    }

    #[test]
    fn test_http_method_custom() {
        let method = HttpMethod::Custom("CONNECT".to_string());
        assert_eq!(method.as_str(), "CONNECT");
    }

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
    fn test_find_header() {
        let headers = vec![
            "Content-Type: application/json".to_string(),
            "Content-Length: 123".to_string(),
        ];
        assert_eq!(
            find_header(&headers, "content-type"),
            Some("application/json".to_string())
        );
        assert_eq!(
            find_header(&headers, "Content-Length"),
            Some("123".to_string())
        );
        assert_eq!(find_header(&headers, "Authorization"), None);
    }

    #[test]
    fn test_find_header_with_whitespace() {
        let headers = vec!["  Content-Type  :  application/json  ".to_string()];
        assert_eq!(
            find_header(&headers, "content-type"),
            Some("application/json".to_string())
        );
    }

    #[test]
    fn test_parse_headers() {
        let headers = vec![
            "Content-Type: application/json".to_string(),
            "Content-Length: 123".to_string(),
        ];
        let map = parse_headers(&headers);
        assert_eq!(
            map.get("content-type"),
            Some(&"application/json".to_string())
        );
        assert_eq!(map.get("content-length"), Some(&"123".to_string()));
    }

    #[test]
    fn test_parse_headers_skips_malformed() {
        let headers = vec![
            "Content-Type: application/json".to_string(),
            "MalformedHeader".to_string(),
            "Content-Length: 123".to_string(),
        ];
        let map = parse_headers(&headers);
        assert_eq!(map.len(), 2);
        assert!(map.get("malformedheader").is_none());
    }
}
