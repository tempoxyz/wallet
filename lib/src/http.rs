//! HTTP client implementation using curl.

use crate::error::{PurlError, Result};
use curl::easy::{Easy2, Handler, WriteError};
use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

/// Valid HTTP methods (RFC 9110)
const VALID_METHODS: &[&str] = &[
    "GET", "POST", "PUT", "DELETE", "PATCH", "HEAD", "OPTIONS", "TRACE", "CONNECT",
];

/// Validates that an HTTP method is a recognized standard method.
///
/// # Errors
/// Returns an error if the method is not in the list of valid HTTP methods.
pub fn validate_http_method(method: &str) -> Result<()> {
    let upper = method.to_uppercase();
    if !VALID_METHODS.contains(&upper.as_str()) {
        return Err(PurlError::Http(format!(
            "Invalid HTTP method: {}. Valid methods: {:?}",
            method, VALID_METHODS
        )));
    }
    Ok(())
}

/// Validates that a header does not contain CRLF characters to prevent header injection.
///
/// # Errors
/// Returns an error if the header contains `\r` or `\n` characters.
pub fn validate_header(header: &str) -> Result<()> {
    if header.contains('\r') || header.contains('\n') {
        return Err(PurlError::Http(
            "Header contains invalid characters (CRLF injection attempt)".to_string(),
        ));
    }
    Ok(())
}

/// Validates all headers in a slice for CRLF injection.
///
/// # Errors
/// Returns an error if any header contains `\r` or `\n` characters.
pub fn validate_headers(headers: &[String]) -> Result<()> {
    for header in headers {
        validate_header(header)?;
    }
    Ok(())
}

/// Validates header name-value tuples for CRLF injection.
///
/// # Errors
/// Returns an error if any header name or value contains `\r` or `\n` characters.
pub fn validate_header_tuples(headers: &[(String, String)]) -> Result<()> {
    for (name, value) in headers {
        validate_header(name)?;
        validate_header(value)?;
    }
    Ok(())
}

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
    type Err = PurlError;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        validate_http_method(s)?;
        Ok(match s.to_uppercase().as_str() {
            "GET" => HttpMethod::Get,
            "POST" => HttpMethod::Post,
            "PUT" => HttpMethod::Put,
            "PATCH" => HttpMethod::Patch,
            "DELETE" => HttpMethod::Delete,
            "HEAD" => HttpMethod::Head,
            "OPTIONS" => HttpMethod::Options,
            "TRACE" => HttpMethod::Custom("TRACE".to_string()),
            "CONNECT" => HttpMethod::Custom("CONNECT".to_string()),
            _ => unreachable!("validate_http_method should have rejected this"),
        })
    }
}

impl TryFrom<&str> for HttpMethod {
    type Error = PurlError;

    fn try_from(s: &str) -> std::result::Result<Self, Self::Error> {
        s.parse()
    }
}

impl TryFrom<&String> for HttpMethod {
    type Error = PurlError;

    fn try_from(s: &String) -> std::result::Result<Self, Self::Error> {
        s.as_str().parse()
    }
}

impl TryFrom<String> for HttpMethod {
    type Error = PurlError;

    fn try_from(s: String) -> std::result::Result<Self, Self::Error> {
        s.as_str().parse()
    }
}

struct ResponseHandler {
    data: Vec<u8>,
    headers: HashMap<String, String>,
}

impl ResponseHandler {
    fn new() -> Self {
        Self {
            data: Vec::new(),
            headers: HashMap::new(),
        }
    }
}

impl Handler for ResponseHandler {
    fn write(&mut self, data: &[u8]) -> std::result::Result<usize, WriteError> {
        self.data.extend_from_slice(data);
        Ok(data.len())
    }

    fn header(&mut self, header: &[u8]) -> bool {
        if let Ok(header_str) = std::str::from_utf8(header) {
            if let Some((key, value)) = header_str.split_once(':') {
                self.headers
                    .insert(key.trim().to_lowercase(), value.trim().to_string());
            }
        }
        true
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
        // Use from_utf8_lossy to avoid unnecessary clone for valid UTF-8
        // We still need to allocate since we can't consume self
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

    /// Get the payment requirements JSON from either the PAYMENT-REQUIRED header (base64) or body.
    ///
    /// For x402 v2, payment requirements are sent in the PAYMENT-REQUIRED header (base64 encoded).
    /// For backwards compatibility with v1, this also falls back to the response body.
    ///
    /// # Errors
    /// Returns an error if the header is present but cannot be decoded, or if the body is not valid UTF-8.
    pub fn payment_requirements_json(&self) -> Result<String> {
        crate::protocol::x402::payment_requirements_json(self)
    }
}

/// Builder for configuring HTTP clients.
///
/// This provides a fluent API for setting up an HttpClient with various options.
#[must_use]
pub struct HttpClientBuilder {
    verbose: bool,
    timeout: Option<u64>,
    follow_redirects: bool,
    user_agent: Option<String>,
    headers: Vec<(String, String)>,
}

impl HttpClientBuilder {
    /// Create a new HTTP client builder with default settings.
    pub fn new() -> Self {
        Self {
            verbose: false,
            timeout: None,
            follow_redirects: false,
            user_agent: None,
            headers: Vec::new(),
        }
    }

    /// Enable verbose output for debugging.
    pub fn verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    /// Set request timeout in seconds.
    pub fn timeout(mut self, seconds: u64) -> Self {
        self.timeout = Some(seconds);
        self
    }

    /// Enable following HTTP redirects.
    pub fn follow_redirects(mut self, follow: bool) -> Self {
        self.follow_redirects = follow;
        self
    }

    /// Set custom User-Agent header.
    pub fn user_agent(mut self, ua: impl Into<String>) -> Self {
        self.user_agent = Some(ua.into());
        self
    }

    /// Add a custom HTTP header.
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.push((name.into(), value.into()));
        self
    }

    /// Add multiple headers at once.
    pub fn headers(mut self, headers: &[(String, String)]) -> Self {
        self.headers.extend_from_slice(headers);
        self
    }

    /// Build the configured HTTP client.
    pub fn build(self) -> Result<HttpClient> {
        let mut client = HttpClient::new()?;

        if self.verbose {
            client.set_verbose(true)?;
        }

        if let Some(timeout) = self.timeout {
            client.set_timeout(timeout)?;
        }

        if self.follow_redirects {
            client.set_follow_location(true)?;
        }

        if let Some(ref ua) = self.user_agent {
            client.set_user_agent(ua)?;
        }

        if !self.headers.is_empty() {
            client.set_headers(&self.headers)?;
        }

        Ok(client)
    }
}

impl Default for HttpClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}

pub struct HttpClient {
    curl: Easy2<ResponseHandler>,
}

impl HttpClient {
    pub fn new() -> Result<Self> {
        let handler = ResponseHandler::new();
        let curl = Easy2::new(handler);

        Ok(Self { curl })
    }

    pub fn set_headers(&mut self, headers: &[(String, String)]) -> Result<()> {
        validate_header_tuples(headers)?;
        let mut list = curl::easy::List::new();
        for (name, value) in headers {
            list.append(&format!("{name}: {value}"))?;
        }
        self.curl.http_headers(list)?;
        Ok(())
    }

    /// Set verbose mode
    pub fn set_verbose(&mut self, verbose: bool) -> Result<()> {
        self.curl.verbose(verbose)?;
        Ok(())
    }

    /// Set timeout
    pub fn set_timeout(&mut self, timeout_secs: u64) -> Result<()> {
        self.curl
            .timeout(std::time::Duration::from_secs(timeout_secs))?;
        Ok(())
    }

    /// Set follow redirects
    pub fn set_follow_location(&mut self, follow: bool) -> Result<()> {
        self.curl.follow_location(follow)?;
        Ok(())
    }

    /// Set user agent
    pub fn set_user_agent(&mut self, user_agent: &str) -> Result<()> {
        self.curl.useragent(user_agent)?;
        Ok(())
    }

    /// Perform a GET request
    pub fn get(&mut self, url: &str) -> Result<HttpResponse> {
        self.curl.url(url)?;
        self.curl.get(true)?;
        self.perform()
    }

    /// Perform a POST request with optional body
    pub fn post(&mut self, url: &str, body: Option<&[u8]>) -> Result<HttpResponse> {
        self.curl.url(url)?;
        self.curl.post(true)?;

        if let Some(data) = body {
            self.curl.post_field_size(data.len() as u64)?;
            self.curl.post_fields_copy(data)?;
        }

        self.perform()
    }

    /// Perform a request with the specified HTTP method and optional body.
    ///
    /// This method accepts any type that implements `Into<HttpMethod>`, including
    /// `&str` for convenience.
    pub fn request(
        &mut self,
        method: impl Into<HttpMethod>,
        url: &str,
        body: Option<&[u8]>,
    ) -> Result<HttpResponse> {
        let method = method.into();
        self.curl.url(url)?;

        match &method {
            HttpMethod::Get => {
                self.curl.get(true)?;
            }
            HttpMethod::Post => {
                self.curl.post(true)?;
                if let Some(data) = body {
                    self.curl.post_field_size(data.len() as u64)?;
                    self.curl.post_fields_copy(data)?;
                }
            }
            HttpMethod::Put => {
                self.curl.custom_request("PUT")?;
                if let Some(data) = body {
                    self.curl.post_field_size(data.len() as u64)?;
                    self.curl.post_fields_copy(data)?;
                }
            }
            HttpMethod::Patch => {
                self.curl.custom_request("PATCH")?;
                if let Some(data) = body {
                    self.curl.post_field_size(data.len() as u64)?;
                    self.curl.post_fields_copy(data)?;
                }
            }
            HttpMethod::Delete => {
                self.curl.custom_request("DELETE")?;
                if let Some(data) = body {
                    self.curl.post_field_size(data.len() as u64)?;
                    self.curl.post_fields_copy(data)?;
                }
            }
            HttpMethod::Head => {
                self.curl.nobody(true)?;
            }
            HttpMethod::Options => {
                self.curl.custom_request("OPTIONS")?;
            }
            HttpMethod::Custom(name) => {
                self.curl.custom_request(name)?;
                if let Some(data) = body {
                    self.curl.post_field_size(data.len() as u64)?;
                    self.curl.post_fields_copy(data)?;
                }
            }
        }

        self.perform()
    }

    /// Perform the request and return the response
    fn perform(&mut self) -> Result<HttpResponse> {
        self.curl.perform()?;

        let status_code = self.curl.response_code()?;

        let handler = self.curl.get_mut();

        // Take ownership of data instead of cloning
        let response = HttpResponse {
            status_code,
            headers: std::mem::take(&mut handler.headers),
            body: std::mem::take(&mut handler.data),
        };

        Ok(response)
    }
}

// =============================================================================
// Header Utilities
// =============================================================================

/// Check if a header with the given name exists in a list of raw header strings.
///
/// Headers are expected in "Name: Value" format. The comparison is case-insensitive
/// for the header name, as per HTTP specification.
///
/// # Example
///
/// ```
/// use purl_lib::http::has_header;
///
/// let headers = vec!["Content-Type: application/json".to_string(), "Authorization: Bearer token".to_string()];
/// assert!(has_header(&headers, "content-type"));
/// assert!(has_header(&headers, "Content-Type"));
/// assert!(!has_header(&headers, "Accept"));
/// ```
pub fn has_header(headers: &[String], name: &str) -> bool {
    headers.iter().any(|h| {
        h.split_once(':')
            .map(|(k, _)| k.trim().eq_ignore_ascii_case(name))
            .unwrap_or(false)
    })
}

/// Find and return the value of a header by name from a list of raw header strings.
///
/// Headers are expected in "Name: Value" format. The comparison is case-insensitive
/// for the header name. Returns the first matching header's value (trimmed).
///
/// # Example
///
/// ```
/// use purl_lib::http::find_header;
///
/// let headers = vec!["Content-Type: application/json".to_string()];
/// assert_eq!(find_header(&headers, "content-type"), Some("application/json"));
/// assert_eq!(find_header(&headers, "Accept"), None);
/// ```
pub fn find_header<'a>(headers: &'a [String], name: &str) -> Option<&'a str> {
    headers.iter().find_map(|h| {
        h.split_once(':').and_then(|(k, v)| {
            if k.trim().eq_ignore_ascii_case(name) {
                Some(v.trim())
            } else {
                None
            }
        })
    })
}

/// Parse raw header strings into (name, value) tuples.
///
/// Headers without a colon are silently skipped. Both name and value are trimmed.
///
/// # Example
///
/// ```
/// use purl_lib::http::parse_headers;
///
/// let raw = vec!["Content-Type: application/json".to_string(), "Accept: */*".to_string()];
/// let parsed = parse_headers(&raw);
/// assert_eq!(parsed, vec![
///     ("Content-Type".to_string(), "application/json".to_string()),
///     ("Accept".to_string(), "*/*".to_string()),
/// ]);
/// ```
pub fn parse_headers(headers: &[String]) -> Vec<(String, String)> {
    headers
        .iter()
        .filter_map(|h| {
            h.split_once(':')
                .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_method_from_str() {
        assert_eq!("GET".parse::<HttpMethod>().unwrap(), HttpMethod::Get);
        assert_eq!("POST".parse::<HttpMethod>().unwrap(), HttpMethod::Post);
        assert_eq!("PUT".parse::<HttpMethod>().unwrap(), HttpMethod::Put);
        assert_eq!("PATCH".parse::<HttpMethod>().unwrap(), HttpMethod::Patch);
        assert_eq!("DELETE".parse::<HttpMethod>().unwrap(), HttpMethod::Delete);
        assert_eq!("HEAD".parse::<HttpMethod>().unwrap(), HttpMethod::Head);
        assert_eq!(
            "OPTIONS".parse::<HttpMethod>().unwrap(),
            HttpMethod::Options
        );
    }

    #[test]
    fn test_http_method_from_str_case_insensitive() {
        assert_eq!("get".parse::<HttpMethod>().unwrap(), HttpMethod::Get);
        assert_eq!("Get".parse::<HttpMethod>().unwrap(), HttpMethod::Get);
        assert_eq!("post".parse::<HttpMethod>().unwrap(), HttpMethod::Post);
        assert_eq!("Post".parse::<HttpMethod>().unwrap(), HttpMethod::Post);
        assert_eq!("delete".parse::<HttpMethod>().unwrap(), HttpMethod::Delete);
    }

    #[test]
    fn test_http_method_custom() {
        let method: HttpMethod = "CONNECT".parse().unwrap();
        assert_eq!(method, HttpMethod::Custom("CONNECT".to_string()));

        let method: HttpMethod = "TRACE".parse().unwrap();
        assert_eq!(method, HttpMethod::Custom("TRACE".to_string()));
    }

    #[test]
    fn test_http_method_invalid_rejected() {
        assert!("custom".parse::<HttpMethod>().is_err());
        assert!("INVALID".parse::<HttpMethod>().is_err());
        assert!("".parse::<HttpMethod>().is_err());
    }

    #[test]
    fn test_http_method_as_str() {
        assert_eq!(HttpMethod::Get.as_str(), "GET");
        assert_eq!(HttpMethod::Post.as_str(), "POST");
        assert_eq!(HttpMethod::Put.as_str(), "PUT");
        assert_eq!(HttpMethod::Patch.as_str(), "PATCH");
        assert_eq!(HttpMethod::Delete.as_str(), "DELETE");
        assert_eq!(HttpMethod::Head.as_str(), "HEAD");
        assert_eq!(HttpMethod::Options.as_str(), "OPTIONS");
        assert_eq!(
            HttpMethod::Custom("CONNECT".to_string()).as_str(),
            "CONNECT"
        );
    }

    #[test]
    fn test_http_method_display() {
        assert_eq!(format!("{}", HttpMethod::Get), "GET");
        assert_eq!(format!("{}", HttpMethod::Post), "POST");
        assert_eq!(
            format!("{}", HttpMethod::Custom("TRACE".to_string())),
            "TRACE"
        );
    }

    #[test]
    fn test_http_method_has_body() {
        // Methods that typically have a body
        assert!(HttpMethod::Post.has_body());
        assert!(HttpMethod::Put.has_body());
        assert!(HttpMethod::Patch.has_body());
        assert!(HttpMethod::Custom("CUSTOM".to_string()).has_body());

        // Methods that typically don't have a body
        assert!(!HttpMethod::Get.has_body());
        assert!(!HttpMethod::Delete.has_body());
        assert!(!HttpMethod::Head.has_body());
        assert!(!HttpMethod::Options.has_body());
    }

    #[test]
    fn test_http_method_default() {
        assert_eq!(HttpMethod::default(), HttpMethod::Get);
    }

    #[test]
    fn test_http_method_parse() {
        let method: HttpMethod = "POST".parse().unwrap();
        assert_eq!(method, HttpMethod::Post);

        let method: HttpMethod = "put".parse().unwrap();
        assert_eq!(method, HttpMethod::Put);
    }

    #[test]
    fn test_http_method_equality() {
        assert_eq!(HttpMethod::Get, HttpMethod::Get);
        assert_ne!(HttpMethod::Get, HttpMethod::Post);
        assert_eq!(
            HttpMethod::Custom("FOO".to_string()),
            HttpMethod::Custom("FOO".to_string())
        );
        assert_ne!(
            HttpMethod::Custom("FOO".to_string()),
            HttpMethod::Custom("BAR".to_string())
        );
    }

    #[test]
    fn test_http_method_clone() {
        let method = HttpMethod::Post;
        let cloned = method.clone();
        assert_eq!(method, cloned);

        let custom = HttpMethod::Custom("TEST".to_string());
        let cloned_custom = custom.clone();
        assert_eq!(custom, cloned_custom);
    }

    #[test]
    fn test_http_method_hash() {
        use std::collections::HashSet;

        let mut set = HashSet::new();
        set.insert(HttpMethod::Get);
        set.insert(HttpMethod::Post);
        set.insert(HttpMethod::Get); // Duplicate

        assert_eq!(set.len(), 2);
        assert!(set.contains(&HttpMethod::Get));
        assert!(set.contains(&HttpMethod::Post));
    }

    #[test]
    fn test_has_header() {
        let headers = vec![
            "Content-Type: application/json".to_string(),
            "Authorization: Bearer token".to_string(),
        ];

        assert!(has_header(&headers, "Content-Type"));
        assert!(has_header(&headers, "content-type"));
        assert!(has_header(&headers, "CONTENT-TYPE"));
        assert!(has_header(&headers, "Authorization"));
        assert!(!has_header(&headers, "Accept"));
        assert!(!has_header(&headers, ""));
    }

    #[test]
    fn test_has_header_empty() {
        let headers: Vec<String> = vec![];
        assert!(!has_header(&headers, "Content-Type"));
    }

    #[test]
    fn test_has_header_malformed() {
        let headers = vec![
            "NoColonHeader".to_string(),
            "Content-Type: application/json".to_string(),
        ];

        assert!(has_header(&headers, "Content-Type"));
        assert!(!has_header(&headers, "NoColonHeader"));
    }

    #[test]
    fn test_find_header() {
        let headers = vec![
            "Content-Type: application/json".to_string(),
            "Authorization: Bearer token".to_string(),
        ];

        assert_eq!(
            find_header(&headers, "Content-Type"),
            Some("application/json")
        );
        assert_eq!(
            find_header(&headers, "content-type"),
            Some("application/json")
        );
        assert_eq!(find_header(&headers, "Authorization"), Some("Bearer token"));
        assert_eq!(find_header(&headers, "Accept"), None);
    }

    #[test]
    fn test_find_header_with_whitespace() {
        let headers = vec!["  Content-Type  :  application/json  ".to_string()];

        assert_eq!(
            find_header(&headers, "Content-Type"),
            Some("application/json")
        );
    }

    #[test]
    fn test_parse_headers() {
        let raw = vec![
            "Content-Type: application/json".to_string(),
            "Accept: */*".to_string(),
        ];
        let parsed = parse_headers(&raw);
        assert_eq!(
            parsed,
            vec![
                ("Content-Type".to_string(), "application/json".to_string()),
                ("Accept".to_string(), "*/*".to_string()),
            ]
        );
    }

    #[test]
    fn test_parse_headers_skips_malformed() {
        let raw = vec![
            "Valid: header".to_string(),
            "NoColon".to_string(),
            "Another: one".to_string(),
        ];
        let parsed = parse_headers(&raw);
        assert_eq!(
            parsed,
            vec![
                ("Valid".to_string(), "header".to_string()),
                ("Another".to_string(), "one".to_string()),
            ]
        );
    }

    #[test]
    fn test_validate_http_method_valid() {
        assert!(validate_http_method("GET").is_ok());
        assert!(validate_http_method("get").is_ok());
        assert!(validate_http_method("Post").is_ok());
        assert!(validate_http_method("PUT").is_ok());
        assert!(validate_http_method("DELETE").is_ok());
        assert!(validate_http_method("PATCH").is_ok());
        assert!(validate_http_method("HEAD").is_ok());
        assert!(validate_http_method("OPTIONS").is_ok());
        assert!(validate_http_method("TRACE").is_ok());
        assert!(validate_http_method("CONNECT").is_ok());
    }

    #[test]
    fn test_validate_http_method_invalid() {
        assert!(validate_http_method("INVALID").is_err());
        assert!(validate_http_method("FOO").is_err());
        assert!(validate_http_method("").is_err());
        assert!(validate_http_method("GET\r\nX-Injected: value").is_err());
    }

    #[test]
    fn test_validate_header_valid() {
        assert!(validate_header("Content-Type: application/json").is_ok());
        assert!(validate_header("Authorization: Bearer token").is_ok());
        assert!(validate_header("X-Custom-Header: value with spaces").is_ok());
    }

    #[test]
    fn test_validate_header_crlf_injection() {
        assert!(validate_header("X-Header: value\r\nX-Injected: evil").is_err());
        assert!(validate_header("X-Header: value\nX-Injected: evil").is_err());
        assert!(validate_header("X-Header: value\rX-Injected: evil").is_err());
        assert!(validate_header("\r").is_err());
        assert!(validate_header("\n").is_err());
    }

    #[test]
    fn test_validate_headers_valid() {
        let headers = vec![
            "Content-Type: application/json".to_string(),
            "Accept: */*".to_string(),
        ];
        assert!(validate_headers(&headers).is_ok());
    }

    #[test]
    fn test_validate_headers_with_injection() {
        let headers = vec![
            "Content-Type: application/json".to_string(),
            "X-Evil: value\r\nX-Injected: evil".to_string(),
        ];
        assert!(validate_headers(&headers).is_err());
    }

    #[test]
    fn test_validate_header_tuples_valid() {
        let headers = vec![
            ("Content-Type".to_string(), "application/json".to_string()),
            ("Accept".to_string(), "*/*".to_string()),
        ];
        assert!(validate_header_tuples(&headers).is_ok());
    }

    #[test]
    fn test_validate_header_tuples_injection_in_name() {
        let headers = vec![("X-Evil\r\n".to_string(), "value".to_string())];
        assert!(validate_header_tuples(&headers).is_err());
    }

    #[test]
    fn test_validate_header_tuples_injection_in_value() {
        let headers = vec![("X-Header".to_string(), "value\r\nevil".to_string())];
        assert!(validate_header_tuples(&headers).is_err());
    }

    #[test]
    fn test_http_method_parse_invalid_method() {
        let result: std::result::Result<HttpMethod, _> = "INVALID".parse();
        assert!(result.is_err());
    }

    #[test]
    fn test_http_method_parse_trace_connect() {
        let trace: HttpMethod = "TRACE".parse().unwrap();
        assert_eq!(trace, HttpMethod::Custom("TRACE".to_string()));

        let connect: HttpMethod = "CONNECT".parse().unwrap();
        assert_eq!(connect, HttpMethod::Custom("CONNECT".to_string()));
    }
}
