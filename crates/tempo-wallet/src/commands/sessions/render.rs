//! Session rendering: view models and output rendering (text, JSON).

use alloy::primitives::{utils::format_units, U256};
use serde::Serialize;

use super::{session_store, SessionStatus};
use tempo_common::{
    cli::{
        format::{format_duration, format_relative_time, format_utc_timestamp},
        output,
        output::OutputFormat,
    },
    error::TempoError,
};

// ---------------------------------------------------------------------------
// ChannelView — unified view model for session/channel display
// ---------------------------------------------------------------------------

/// Unified channel view used by all list/display functions.
///
/// Each command builds a `Vec<ChannelView>` with its own business logic,
/// then delegates to [`render_channel_list`] for consistent JSON/text output.
pub(super) struct ChannelView {
    pub(super) channel_id: String,
    pub(super) network: String,
    /// When `Some`, the Channel line is shown in text output.
    /// Non-empty values are used as the header; empty values fall back to `channel_id`.
    /// When `None`, `channel_id` is the header and no Channel line is shown.
    pub(super) origin: Option<String>,
    pub(super) symbol: &'static str,
    pub(super) unlimited: bool,
    pub(super) deposit: String,
    pub(super) spent: String,
    pub(super) remaining: String,
    pub(super) status: SessionStatus,
    pub(super) remaining_secs: Option<u64>,
    pub(super) created_at: Option<u64>,
    pub(super) last_used_at: Option<u64>,
}

impl ChannelView {
    /// Build a view from on-chain channel data (used by list and info for
    /// channels that don't have a local session record).
    pub(super) fn from_on_chain(
        channel_id: &str,
        network: tempo_common::network::NetworkId,
        deposit: u128,
        settled: u128,
        close_requested_at: u64,
        grace_period: u64,
    ) -> Self {
        let t = network.token();
        let remaining = deposit.saturating_sub(settled);

        let (status, remaining_secs) = if close_requested_at > 0 {
            let now = session_store::now_secs();
            let ready_at = close_requested_at + grace_period;
            let rem = ready_at.saturating_sub(now);
            if rem == 0 {
                (SessionStatus::Finalizable, Some(0))
            } else {
                (SessionStatus::Closing, Some(rem))
            }
        } else {
            (SessionStatus::Orphaned, None)
        };

        Self {
            channel_id: channel_id.to_string(),
            network: network.as_str().to_string(),
            origin: None,
            symbol: t.symbol,
            unlimited: deposit == 0,
            deposit: format_units(U256::from(deposit), t.decimals).expect("decimals <= 77"),
            spent: format_units(U256::from(settled), t.decimals).expect("decimals <= 77"),
            remaining: format_units(U256::from(remaining), t.decimals).expect("decimals <= 77"),
            status,
            remaining_secs,
            created_at: None,
            last_used_at: None,
        }
    }
}

impl From<&session_store::SessionRecord> for ChannelView {
    fn from(session: &session_store::SessionRecord) -> Self {
        let t = session.network_id().token();

        let spent_u = session.cumulative_amount_u128();
        let limit_u = session.deposit_u128();
        let remaining_u = limit_u.saturating_sub(spent_u);

        let (status, remaining_secs) = session.status_at(session_store::now_secs());

        Self {
            channel_id: session.channel_id_hex(),
            network: session.network_id().as_str().to_string(),
            origin: Some(session.origin.clone()),
            symbol: t.symbol,
            unlimited: limit_u == 0,
            deposit: format_units(U256::from(limit_u), t.decimals).expect("decimals <= 77"),
            spent: format_units(U256::from(spent_u), t.decimals).expect("decimals <= 77"),
            remaining: format_units(U256::from(remaining_u), t.decimals).expect("decimals <= 77"),
            status,
            remaining_secs,
            created_at: Some(session.created_at),
            last_used_at: Some(session.last_used_at),
        }
    }
}

// ---------------------------------------------------------------------------
// SessionItem — JSON serialization wrapper
// ---------------------------------------------------------------------------

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
    created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_used_at: Option<String>,
}

#[derive(Serialize)]
struct SessionListResponse<'a> {
    sessions: Vec<SessionItem<'a>>,
    total: usize,
}

// ---------------------------------------------------------------------------
// Channel list rendering
// ---------------------------------------------------------------------------

/// Render a list of channels as JSON or text.
pub(super) fn render_channel_list(
    views: &[ChannelView],
    output_format: OutputFormat,
    empty_msg: &str,
    count_label: &str,
) -> Result<(), TempoError> {
    let items: Vec<SessionItem<'_>> = views
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
            status: v.status.as_str(),
            remaining_secs: v.remaining_secs,
            created_at: v.created_at.map(format_utc_timestamp),
            last_used_at: v.last_used_at.map(format_utc_timestamp),
        })
        .collect();
    let structured_payload = SessionListResponse {
        total: views.len(),
        sessions: items,
    };

    output::emit_by_format(output_format, &structured_payload, || {
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
        Ok(())
    })?;

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
    if v.unlimited {
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
    let status_str = v.status.as_str();
    let status_display = match v.remaining_secs {
        Some(0) => match v.status {
            SessionStatus::Closing | SessionStatus::Finalizable => {
                "finalizable — ready to finalize".to_string()
            }
            _ => format!("{status_str} — ready to finalize"),
        },
        Some(secs) => format!("{status_str} — {} remaining", format_duration(secs)),
        None => status_str.to_string(),
    };
    println!("{:>10}: {}", "Status", status_display);
    println!();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempo_common::cli::output::OutputFormat;

    fn make_channel_view(status: SessionStatus, remaining_secs: Option<u64>) -> ChannelView {
        ChannelView {
            channel_id: "0xabc123".to_string(),
            network: "tempo".to_string(),
            origin: Some("https://api.example.com".to_string()),
            symbol: "USDC",
            unlimited: false,
            deposit: "10.000000".to_string(),
            spent: "3.500000".to_string(),
            remaining: "6.500000".to_string(),
            status,
            remaining_secs,
            created_at: None,
            last_used_at: None,
        }
    }

    // ==================== render_channel_list ====================

    #[test]
    fn test_render_channel_list_json_empty() {
        let views: Vec<ChannelView> = vec![];
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
        let views = vec![make_channel_view(SessionStatus::Active, None)];
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
        let views = vec![make_channel_view(SessionStatus::Active, None)];
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
        let views = vec![make_channel_view(SessionStatus::Closing, Some(120))];
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
        let views = vec![make_channel_view(SessionStatus::Finalizable, Some(0))];
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
        let mut v = make_channel_view(SessionStatus::Orphaned, None);
        v.origin = None;
        let result =
            render_channel_list(&[v], OutputFormat::Text, "No sessions.", "session(s) total");
        assert!(result.is_ok());
    }

    #[test]
    fn test_render_channel_empty_origin_uses_channel_id() {
        let mut v = make_channel_view(SessionStatus::Orphaned, None);
        v.origin = Some(String::new());
        let result =
            render_channel_list(&[v], OutputFormat::Json, "No sessions.", "session(s) total");
        assert!(result.is_ok());
    }
}
