//! HTTP response formatting and display helpers.

/// Format an HTTP status code + reason for error messages.
pub fn format_http_error(status: u16) -> String {
    format!("{} {}", status, http_status_text(status))
}

/// Map an HTTP status code to a short human-readable reason phrase.
pub fn http_status_text(code: u16) -> &'static str {
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
pub fn print_headers(status: u16, headers: &[(String, String)]) {
    println!("HTTP {status}");
    for (name, value) in headers {
        println!("{name}: {value}");
    }
    println!();
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
