use serde::Serialize;

use super::super::OutputFormat;

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
}

impl ChannelView {
    /// Whether the session has no spending limit (deposit is zero).
    pub(super) fn is_unlimited(&self) -> bool {
        self.deposit.parse::<f64>().is_ok_and(|v| v == 0.0)
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
}

/// Tracks the result of batch close operations for consistent output.
///
/// Used by `close_sessions --all`, `close_orphaned_channels`, and
/// `finalize_closed_channels` to accumulate results and render them
/// in a consistent format.
pub(super) struct CloseSummary {
    closed: u32,
    pending: u32,
    failed: u32,
    results: Vec<serde_json::Value>,
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
        OutputFormat::Json => {
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
                })
                .collect();
            println!(
                "{}",
                serde_json::to_string(&serde_json::json!({
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
            println!("{} {count_label}.", views.len());
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
    // Status
    let status_display = match v.remaining_secs {
        Some(0) => format!("{} — ready to finalize", v.status),
        Some(secs) => format!("{} — {} remaining", v.status, format_duration(secs)),
        None => v.status.clone(),
    };
    println!("{:>10}: {}", "Status", status_display);
    println!();
}

impl CloseSummary {
    pub(super) fn new() -> Self {
        Self {
            closed: 0,
            pending: 0,
            failed: 0,
            results: Vec::new(),
        }
    }

    pub(super) fn record_closed(&mut self, result: serde_json::Value) {
        self.closed += 1;
        self.results.push(result);
    }

    pub(super) fn record_pending(&mut self, result: serde_json::Value) {
        self.pending += 1;
        self.results.push(result);
    }

    pub(super) fn record_failed(&mut self, result: serde_json::Value) {
        self.failed += 1;
        self.results.push(result);
    }

    pub(super) fn print(&self, output_format: OutputFormat, empty_msg: &str, closed_label: &str) {
        match output_format {
            OutputFormat::Json => println!(
                "{}",
                serde_json::json!({
                    "closed": self.closed,
                    "pending": self.pending,
                    "failed": self.failed,
                    "results": self.results
                })
            ),
            OutputFormat::Text => {
                let total = self.closed + self.pending + self.failed;
                if total == 0 {
                    println!("{empty_msg}");
                } else {
                    let mut parts = Vec::new();
                    if self.closed > 0 {
                        parts.push(format!("{} {closed_label}", self.closed));
                    }
                    if self.pending > 0 {
                        parts.push(format!("{} pending", self.pending));
                    }
                    if self.failed > 0 {
                        parts.push(format!("{} failed", self.failed));
                    }
                    println!("{}", parts.join(", "));
                }
            }
        }
    }
}

/// Format seconds as a human-readable duration (e.g., "15m 0s", "2m 30s").
pub(super) fn format_duration(secs: u64) -> String {
    if secs >= 60 {
        let m = secs / 60;
        let s = secs % 60;
        if s == 0 {
            format!("{m}m")
        } else {
            format!("{m}m {s}s")
        }
    } else {
        format!("{secs}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_duration_zero() {
        assert_eq!(format_duration(0), "0s");
    }

    #[test]
    fn test_format_duration_seconds() {
        assert_eq!(format_duration(1), "1s");
        assert_eq!(format_duration(59), "59s");
    }

    #[test]
    fn test_format_duration_exact_minutes() {
        assert_eq!(format_duration(60), "1m");
        assert_eq!(format_duration(120), "2m");
        assert_eq!(format_duration(900), "15m");
    }

    #[test]
    fn test_format_duration_minutes_and_seconds() {
        assert_eq!(format_duration(61), "1m 1s");
        assert_eq!(format_duration(90), "1m 30s");
        assert_eq!(format_duration(125), "2m 5s");
    }

    #[test]
    fn test_format_duration_large() {
        assert_eq!(format_duration(3600), "60m");
        assert_eq!(format_duration(3661), "61m 1s");
    }

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

    // ==================== CloseSummary ====================

    #[test]
    fn test_close_summary_empty_text() {
        let summary = CloseSummary::new();
        // Should print empty message, not panic
        summary.print(OutputFormat::Text, "No sessions to close.", "closed");
    }

    #[test]
    fn test_close_summary_empty_json() {
        let summary = CloseSummary::new();
        summary.print(OutputFormat::Json, "No sessions to close.", "closed");
    }

    #[test]
    fn test_close_summary_counts() {
        let mut summary = CloseSummary::new();
        summary.record_closed(serde_json::json!({"channel_id": "0x1", "status": "closed"}));
        summary.record_closed(serde_json::json!({"channel_id": "0x2", "status": "closed"}));
        summary.record_pending(serde_json::json!({"channel_id": "0x3", "status": "pending"}));
        summary.record_failed(serde_json::json!({"channel_id": "0x4", "status": "error"}));

        assert_eq!(summary.closed, 2);
        assert_eq!(summary.pending, 1);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.results.len(), 4);
    }

    #[test]
    fn test_close_summary_json_output_no_panic() {
        let mut summary = CloseSummary::new();
        summary.record_closed(serde_json::json!({"channel_id": "0x1", "status": "closed"}));
        summary.record_pending(
            serde_json::json!({"channel_id": "0x2", "status": "pending", "remaining_secs": 60}),
        );
        summary.record_failed(
            serde_json::json!({"channel_id": "0x3", "status": "error", "error": "timeout"}),
        );
        summary.print(OutputFormat::Json, "No sessions.", "closed");
    }

    #[test]
    fn test_close_summary_text_output_no_panic() {
        let mut summary = CloseSummary::new();
        summary.record_closed(serde_json::json!({"status": "closed"}));
        summary.print(OutputFormat::Text, "No sessions.", "closed");
    }
}
