//! Terminal formatting utilities for address display and ANSI handling.
//!
//! This module provides helpers for formatting blockchain addresses with
//! clickable hyperlinks and handling ANSI escape sequences in terminal output.

use crate::network::explorer::ExplorerConfig;
use crate::util::helpers::truncate_address;

use super::hyperlink::hyperlink;

/// Format an address as a clickable hyperlink if explorer is available.
pub fn format_address_link(address: &str, explorer: Option<&ExplorerConfig>) -> String {
    if let Some(exp) = explorer {
        let url = exp.address_url(address);
        hyperlink(address, &url)
    } else {
        address.to_string()
    }
}

/// Format and truncate an address as a clickable hyperlink if explorer is available.
/// The displayed text is truncated but the link points to the full address.
pub fn format_truncated_address_link(
    address: &str,
    max_len: usize,
    explorer: Option<&ExplorerConfig>,
) -> String {
    let truncated = truncate_address(address, max_len);
    if let Some(exp) = explorer {
        let url = exp.address_url(address);
        hyperlink(&truncated, &url)
    } else {
        truncated
    }
}

/// Pad a string containing possible hyperlink escape codes to a target visible width.
/// Hyperlink escape codes don't contribute to visible width.
pub fn pad_with_hyperlink(s: &str, width: usize) -> String {
    let visible_len = strip_ansi_codes_len(s);
    if visible_len >= width {
        s.to_string()
    } else {
        format!("{}{}", s, " ".repeat(width - visible_len))
    }
}

/// Count the visible length of a string, excluding ANSI escape codes.
pub fn strip_ansi_codes_len(s: &str) -> usize {
    let mut len = 0;
    let mut in_escape = false;
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\x1b' {
            in_escape = true;
            // Check for OSC sequence (ESC ])
            if chars.peek() == Some(&']') {
                chars.next(); // consume ]
                              // Skip until BEL (\x07) or ST (ESC \)
                while let Some(c2) = chars.next() {
                    if c2 == '\x07' {
                        break;
                    }
                    if c2 == '\x1b' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
                in_escape = false;
            }
        } else if in_escape {
            // CSI sequence ends at letter
            if c.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else {
            len += 1;
        }
    }
    len
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_ansi_codes_len_plain_text() {
        assert_eq!(strip_ansi_codes_len("hello"), 5);
        assert_eq!(strip_ansi_codes_len("0x1234567890"), 12);
    }

    #[test]
    fn test_strip_ansi_codes_len_with_osc8() {
        // OSC 8 hyperlink format: ESC ] 8 ; ; URL BEL text ESC ] 8 ; ; BEL
        let hyperlink = "\x1b]8;;https://example.com\x07click me\x1b]8;;\x07";
        assert_eq!(strip_ansi_codes_len(hyperlink), 8); // "click me" = 8 chars
    }

    #[test]
    fn test_pad_with_hyperlink_plain() {
        let result = pad_with_hyperlink("hello", 10);
        assert_eq!(result, "hello     ");
    }

    #[test]
    fn test_pad_with_hyperlink_with_escape() {
        let hyperlink = "\x1b]8;;https://example.com\x07text\x1b]8;;\x07";
        let result = pad_with_hyperlink(hyperlink, 10);
        // "text" is 4 chars, so we need 6 spaces
        assert_eq!(
            result,
            "\x1b]8;;https://example.com\x07text\x1b]8;;\x07      "
        );
    }
}
