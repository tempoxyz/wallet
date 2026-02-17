//! Terminal formatting utilities for address display and ANSI handling.
//!
//! This module provides helpers for formatting blockchain addresses with
//! clickable hyperlinks and handling ANSI escape sequences in terminal output.

use crate::network::explorer::ExplorerConfig;

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
