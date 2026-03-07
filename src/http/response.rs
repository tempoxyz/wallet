//! HTTP response type.

use anyhow::Result;

#[derive(Debug)]
pub(crate) struct HttpResponse {
    pub(crate) status_code: u16,
    /// Response headers stored as name-value pairs with **lowercased** names.
    ///
    /// Header names are normalized to lowercase during conversion.
    /// Duplicate header names (e.g., multiple `Set-Cookie`) are preserved.
    /// Use [`header`](Self::header) for lookup by lowercase name (returns the last value).
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Vec<u8>,
    /// The final URL after following any redirects.
    pub(crate) final_url: Option<String>,
}

/// Extract response headers as lowercased name-value pairs, skipping non-UTF-8 values.
pub(crate) fn extract_headers(headers: &reqwest::header::HeaderMap) -> Vec<(String, String)> {
    headers
        .iter()
        .filter_map(|(k, v)| {
            v.to_str()
                .ok()
                .map(|s| (k.as_str().to_lowercase(), s.to_string()))
        })
        .collect()
}

impl HttpResponse {
    /// Convert a reqwest response into an `HttpResponse`.
    pub(crate) async fn from_reqwest(response: reqwest::Response) -> Result<Self> {
        let status_code = response.status().as_u16();
        let final_url = Some(response.url().to_string());
        let headers = extract_headers(response.headers());
        let body = response.bytes().await?.to_vec();

        Ok(Self {
            status_code,
            headers,
            body,
            final_url,
        })
    }

    /// Convert the response body to a UTF-8 string.
    ///
    /// # Errors
    /// Returns an error if the body is not valid UTF-8.
    pub(crate) fn body_string(&self) -> Result<String> {
        Ok(std::str::from_utf8(&self.body)?.to_string())
    }

    /// Look up a header value by name.
    ///
    /// Header names are stored lowercase; pass a lowercase key.
    pub(crate) fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .rev()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// Format an HTTP status code + reason for error messages.
pub(crate) fn format_http_error(status: u16) -> String {
    format!("{} {}", status, http_status_text(status))
}

/// Map an HTTP status code to a short human-readable reason phrase.
pub(crate) fn http_status_text(code: u16) -> &'static str {
    match code {
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Error",
    }
}

/// Print HTTP status line and headers to stdout.
pub(crate) fn print_headers(status: u16, headers: &[(String, String)]) {
    println!("HTTP {status}");
    for (name, value) in headers {
        println!("{name}: {value}");
    }
    println!();
}

#[cfg(test)]
impl HttpResponse {
    /// Create a test response with the given status and body.
    pub(crate) fn for_test(status: u16, body: &[u8]) -> Self {
        Self {
            status_code: status,
            headers: Vec::new(),
            body: body.to_vec(),
            final_url: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_status_text_known_codes() {
        assert_eq!(http_status_text(400), "Bad Request");
        assert_eq!(http_status_text(401), "Unauthorized");
        assert_eq!(http_status_text(403), "Forbidden");
        assert_eq!(http_status_text(404), "Not Found");
        assert_eq!(http_status_text(429), "Too Many Requests");
        assert_eq!(http_status_text(500), "Internal Server Error");
        assert_eq!(http_status_text(502), "Bad Gateway");
        assert_eq!(http_status_text(503), "Service Unavailable");
        assert_eq!(http_status_text(504), "Gateway Timeout");
    }

    #[test]
    fn test_http_status_text_unknown_code() {
        assert_eq!(http_status_text(418), "Error");
        assert_eq!(http_status_text(599), "Error");
    }
}
