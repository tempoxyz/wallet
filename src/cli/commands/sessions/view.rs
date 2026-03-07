//! Session view model and display rendering (text and JSON output).

use alloy::primitives::utils::format_units;
use alloy::primitives::U256;
use serde::Serialize;

use crate::cli::OutputFormat;
use crate::payment::session::store as session_store;
use crate::util::{format_duration, format_relative_time};

/// Unified channel view used by all list/display functions.
///
/// Each list function builds a `Vec<ChannelView>` with its own business logic,
/// then delegates to `render_channel_list` for consistent JSON/text output.
pub(super) struct ChannelView {
    pub(super) channel_id: String,
    pub(super) network: String,
    /// When `Some`, the Channel line is shown in text output.
    /// Non-empty values are used as the header; empty values fall back to channel_id.
    /// When `None`, channel_id is the header and no Channel line is shown.
    pub(super) origin: Option<String>,
    pub(super) symbol: &'static str,
    pub(super) deposit: String,
    pub(super) spent: String,
    pub(super) remaining: String,
    pub(super) status: String,
    pub(super) remaining_secs: Option<u64>,
    pub(super) created_at: Option<u64>,
    pub(super) last_used_at: Option<u64>,
}

impl ChannelView {
    /// Whether the session has no spending limit (deposit is zero).
    pub(super) fn is_unlimited(&self) -> bool {
        self.deposit.trim_end_matches('0').trim_end_matches('.') == "0"
    }

    /// Build a view from on-chain channel data (used by list and info for
    /// channels that don't have a local session record).
    pub(super) fn from_on_chain(
        channel_id: &str,
        network: crate::network::NetworkId,
        deposit: u128,
        settled: u128,
        close_requested_at: u64,
        grace_period: u64,
    ) -> Self {
        let t = network.token();
        let remaining = deposit.saturating_sub(settled);

        let (status, remaining_secs) = if close_requested_at > 0 {
            let now = crate::payment::session::store::now_secs();
            let ready_at = close_requested_at + grace_period;
            let rem = ready_at.saturating_sub(now);
            if rem == 0 {
                (crate::payment::session::store::SessionStatus::Finalizable, Some(0))
            } else {
                (crate::payment::session::store::SessionStatus::Closing, Some(rem))
            }
        } else {
            (crate::payment::session::store::SessionStatus::Orphaned, None)
        };

        ChannelView {
            channel_id: channel_id.to_string(),
            network: network.as_str().to_string(),
            origin: None,
            symbol: t.symbol,
            deposit: alloy::primitives::utils::format_units(
                alloy::primitives::U256::from(deposit),
                t.decimals,
            )
            .expect("decimals <= 77"),
            spent: alloy::primitives::utils::format_units(
                alloy::primitives::U256::from(settled),
                t.decimals,
            )
            .expect("decimals <= 77"),
            remaining: alloy::primitives::utils::format_units(
                alloy::primitives::U256::from(remaining),
                t.decimals,
            )
            .expect("decimals <= 77"),
            status: status.as_str().to_string(),
            remaining_secs,
            created_at: None,
            last_used_at: None,
        }
    }
}

/// Shared JSON item for session/channel listings.
///
/// Replaces per-function inline `Item` structs with a single type
/// used by `render_channel_list`.
#[derive(Serialize)]
struct SessionItem<'a> {
    channel_id: &'a str,
    network: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    origin: Option<&'a str>,
    symbol: &'a str,
    deposit: &'a str,
    spent: &'a str,
    remaining: &'a str,
    status: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    remaining_secs: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    created_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_used_at: Option<u64>,
}

// ---------------------------------------------------------------------------
// Rendering helpers
// ---------------------------------------------------------------------------

/// Render a list of channels as JSON or text.
pub(super) fn render_channel_list(
    views: &[ChannelView],
    output_format: OutputFormat,
    empty_msg: &str,
    count_label: &str,
) -> anyhow::Result<()> {
    match output_format {
        OutputFormat::Json | OutputFormat::Toon => {
            let items: Vec<SessionItem> = views
                .iter()
                .map(|v| SessionItem {
                    channel_id: &v.channel_id,
                    network: &v.network,
                    origin: match &v.origin {
                        Some(o) if !o.is_empty() => Some(o.as_str()),
                        _ => None,
                    },
                    symbol: v.symbol,
                    deposit: &v.deposit,
                    spent: &v.spent,
                    remaining: &v.remaining,
                    status: &v.status,
                    remaining_secs: v.remaining_secs,
                    created_at: v.created_at,
                    last_used_at: v.last_used_at,
                })
                .collect();
            println!(
                "{}",
                output_format.serialize(&serde_json::json!({
                    "sessions": items,
                    "total": items.len(),
                }))?
            );
        }
        OutputFormat::Text => {
            if views.is_empty() {
                println!("{empty_msg}");
                return Ok(());
            }
            for v in views {
                render_channel_text(v);
            }
            if !count_label.is_empty() {
                println!("{} {count_label}.", views.len());
            }
        }
    }
    Ok(())
}

/// Render a single channel in text format.
fn render_channel_text(v: &ChannelView) {
    // Header: use origin if available and non-empty, otherwise channel_id
    match &v.origin {
        Some(origin) if !origin.is_empty() => println!("{origin}"),
        _ => println!("{}", v.channel_id),
    }
    println!("{:>10}: {}", "Network", v.network);
    // Show Channel line when origin context is present
    if v.origin.is_some() {
        println!("{:>10}: {}", "Channel", v.channel_id);
    }
    // Amounts
    if v.is_unlimited() {
        println!("{:>10}: unlimited", "Deposit");
    } else {
        let w = [v.deposit.len(), v.spent.len(), v.remaining.len()]
            .into_iter()
            .max()
            .unwrap_or(0);
        println!("{:>10}: {:>w$} {}", "Deposit", v.deposit, v.symbol);
        println!("{:>10}: {:>w$} {}", "Spent", v.spent, v.symbol);
        println!("{:>10}: {:>w$} {}", "Remaining", v.remaining, v.symbol);
    }
    // Timestamps
    if let Some(ts) = v.created_at {
        println!("{:>10}: {}", "Created", format_relative_time(ts));
    }
    if let Some(ts) = v.last_used_at {
        println!("{:>10}: {}", "Last used", format_relative_time(ts));
    }
    // Status
    let status_display = match v.remaining_secs {
        Some(0) => match v.status.as_str() {
            "closing" | "closed" | "finalizable" => "finalizable — ready to finalize".to_string(),
            other => format!("{} — ready to finalize", other),
        },
        Some(secs) => format!("{} — {} remaining", v.status, format_duration(secs)),
        None => v.status.clone(),
    };
    println!("{:>10}: {}", "Status", status_display);
    println!();
}

impl From<&session_store::SessionRecord> for ChannelView {
    fn from(session: &session_store::SessionRecord) -> Self {
        let t = session.network_id().token();

        let spent_u = session.cumulative_amount_u128().unwrap_or(0);
        let limit_u = session.deposit_u128().unwrap_or(0);
        let remaining_u = limit_u.saturating_sub(spent_u);

        let (status, remaining_secs) = session.status_at(session_store::now_secs());

        ChannelView {
            channel_id: session.channel_id.clone(),
            network: session.network_name.clone(),
            origin: Some(session.origin.clone()),
            symbol: t.symbol,
            deposit: format_units(U256::from(limit_u), t.decimals).expect("decimals <= 77"),
            spent: format_units(U256::from(spent_u), t.decimals).expect("decimals <= 77"),
            remaining: format_units(U256::from(remaining_u), t.decimals).expect("decimals <= 77"),
            status: status.as_str().to_string(),
            remaining_secs,
            created_at: Some(session.created_at),
            last_used_at: Some(session.last_used_at),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== ChannelView ====================

    fn make_channel_view(status: &str, remaining_secs: Option<u64>) -> ChannelView {
        ChannelView {
            channel_id: "0xabc123".to_string(),
            network: "tempo".to_string(),
            origin: Some("https://api.example.com".to_string()),
            symbol: "USDC",
            deposit: "10.000000".to_string(),
            spent: "3.500000".to_string(),
            remaining: "6.500000".to_string(),
            status: status.to_string(),
            remaining_secs,
            created_at: None,
            last_used_at: None,
        }
    }

    #[test]
    fn test_channel_view_is_unlimited_zero_deposit() {
        let mut v = make_channel_view("active", None);
        v.deposit = "0.000000".to_string();
        assert!(v.is_unlimited());
    }

    #[test]
    fn test_channel_view_is_unlimited_nonzero_deposit() {
        let v = make_channel_view("active", None);
        assert!(!v.is_unlimited());
    }

    #[test]
    fn test_channel_view_is_unlimited_invalid_deposit() {
        let mut v = make_channel_view("active", None);
        v.deposit = "not-a-number".to_string();
        assert!(!v.is_unlimited());
    }

    // ==================== render_channel_list ====================

    #[test]
    fn test_render_channel_list_json_empty() {
        let views: Vec<ChannelView> = vec![];
        // Capture stdout by redirecting — just verify it doesn't panic
        let result = render_channel_list(
            &views,
            OutputFormat::Json,
            "No sessions.",
            "session(s) total",
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_render_channel_list_text_empty() {
        let views: Vec<ChannelView> = vec![];
        let result = render_channel_list(
            &views,
            OutputFormat::Text,
            "No sessions.",
            "session(s) total",
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_render_channel_list_json_with_entries() {
        let views = vec![make_channel_view("active", None)];
        let result = render_channel_list(
            &views,
            OutputFormat::Json,
            "No sessions.",
            "session(s) total",
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_render_channel_list_text_with_entries() {
        let views = vec![make_channel_view("active", None)];
        let result = render_channel_list(
            &views,
            OutputFormat::Text,
            "No sessions.",
            "session(s) total",
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_render_channel_list_with_closed_status() {
        let views = vec![make_channel_view("closed", Some(120))];
        let result = render_channel_list(
            &views,
            OutputFormat::Text,
            "No sessions.",
            "session(s) total",
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_render_channel_list_ready_to_finalize() {
        let views = vec![make_channel_view("closed", Some(0))];
        let result = render_channel_list(
            &views,
            OutputFormat::Text,
            "No sessions.",
            "session(s) total",
        );
        assert!(result.is_ok());
    }

    #[test]
    fn test_render_channel_no_origin_uses_channel_id() {
        let mut v = make_channel_view("orphaned", None);
        v.origin = None;
        let result =
            render_channel_list(&[v], OutputFormat::Text, "No sessions.", "session(s) total");
        assert!(result.is_ok());
    }

    #[test]
    fn test_render_channel_empty_origin_uses_channel_id() {
        let mut v = make_channel_view("orphaned", None);
        v.origin = Some(String::new());
        let result =
            render_channel_list(&[v], OutputFormat::Json, "No sessions.", "session(s) total");
        assert!(result.is_ok());
    }

}
