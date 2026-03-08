//! Header parsing, validation, and content-type detection for CLI inputs.

use anyhow::Result;

use tempo_common::error::TempoError;

/// Maximum header size (8 KB)
const MAX_HEADER_SIZE: usize = 8 * 1024;

/// Reject a raw header string that exceeds the maximum allowed size.
pub(super) fn validate_header_size(header: &str) -> Result<()> {
    if header.len() > MAX_HEADER_SIZE {
        anyhow::bail!(TempoError::HeaderTooLarge(MAX_HEADER_SIZE));
    }
    Ok(())
}

/// Check if a header name exists in raw header strings (case-insensitive).
pub(super) fn has_header(headers: &[String], name: &str) -> bool {
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
pub(super) fn parse_headers(headers: &[String]) -> Vec<(String, String)> {
    headers
        .iter()
        .filter_map(|header| {
            let (key, value) = header.split_once(':')?;
            Some((key.trim().to_lowercase(), value.trim().to_string()))
        })
        .collect()
}

/// Determine if we should automatically add a JSON Content-Type header.
///
/// Returns true if:
/// - The provided headers don't already contain a Content-Type header, AND
/// - Either json/toon data is provided, OR the first data value looks like JSON
pub(super) fn should_auto_add_json_content_type(
    headers: &[String],
    json: Option<&str>,
    toon: Option<&str>,
    data: &[String],
) -> bool {
    if has_header(headers, "content-type") {
        return false;
    }

    if json.is_some() || toon.is_some() {
        return true;
    }
    if let Some(data) = data.first() {
        if data.starts_with('@') {
            return false;
        }
        let trimmed = data.trim();
        return trimmed.starts_with('{') || trimmed.starts_with('[');
    }
    false
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

    #[test]
    fn test_json_content_type_auto_detection() {
        let no_h: Vec<String> = vec![];
        let json_obj = vec![r#"{"key": "value"}"#.to_string()];
        let json_arr = vec!["[1, 2, 3]".to_string()];
        let json_ws = vec!["  {\"key\": \"value\"}".to_string()];
        let plain = vec!["plain text".to_string()];
        let kv = vec!["key=value".to_string()];
        // JSON-looking data triggers auto-add
        assert!(should_auto_add_json_content_type(
            &no_h, None, None, &json_obj
        ));
        assert!(should_auto_add_json_content_type(
            &no_h, None, None, &json_arr
        ));
        assert!(should_auto_add_json_content_type(
            &no_h, None, None, &json_ws
        ));
        // Non-JSON data does not
        assert!(!should_auto_add_json_content_type(
            &no_h, None, None, &plain
        ));
        assert!(!should_auto_add_json_content_type(&no_h, None, None, &kv));
        // --json flag triggers auto-add
        assert!(should_auto_add_json_content_type(
            &no_h,
            Some(r#"{"key":"value"}"#),
            None,
            &[]
        ));
        // Unrelated headers don't suppress auto-add
        let auth = vec!["Authorization: Bearer token".to_string()];
        assert!(should_auto_add_json_content_type(
            &auth, None, None, &json_obj
        ));
        // Existing Content-Type suppresses auto-add (case-insensitive)
        for ct in [
            "Content-Type: application/json",
            "content-type: application/json",
            "CONTENT-TYPE: application/json",
            "Content-Type: text/plain",
        ] {
            assert!(!should_auto_add_json_content_type(
                &[ct.to_string()],
                None,
                None,
                &json_obj
            ));
        }
    }

    #[test]
    fn test_data_file_does_not_auto_add_json_content_type() {
        let headers: Vec<String> = vec![];
        let data = vec!["@Cargo.toml".to_string()];
        assert!(!should_auto_add_json_content_type(
            &headers, None, None, &data
        ));
    }

    #[test]
    fn test_validate_header_size_limit() {
        let header = format!("X-Big: {}", "a".repeat(MAX_HEADER_SIZE));
        let err = validate_header_size(&header).unwrap_err();
        assert!(
            err.to_string().contains("exceeds maximum size"),
            "got: {err}"
        );
    }

    #[test]
    fn test_should_auto_add_content_type_with_toon() {
        let headers: Vec<String> = vec![];
        assert!(should_auto_add_json_content_type(
            &headers,
            None,
            Some("name: test"),
            &[]
        ));
    }

    #[test]
    fn test_should_not_auto_add_content_type_with_toon_when_user_provides() {
        let headers = vec!["Content-Type: text/plain".to_string()];
        assert!(!should_auto_add_json_content_type(
            &headers,
            None,
            Some("name: test"),
            &[]
        ));
    }
}
