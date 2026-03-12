//! Domain model and helpers for session records.

use anyhow::Context;
use serde::{Deserialize, Serialize};

use crate::network::NetworkId;

/// Session lifecycle state.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum SessionStatus {
    #[default]
    Active,
    Closing,
    Finalizable,
    Finalized,
    Orphaned,
}

impl SessionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Closing => "closing",
            Self::Finalizable => "finalizable",
            Self::Finalized => "finalized",
            Self::Orphaned => "orphaned",
        }
    }

    pub(super) fn from_db_str(value: &str) -> Self {
        match value {
            "active" => Self::Active,
            "closing" => Self::Closing,
            "finalizable" => Self::Finalizable,
            "finalized" => Self::Finalized,
            "orphaned" => Self::Orphaned,
            _ => Self::Active,
        }
    }
}

/// A persisted payment channel session.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SessionRecord {
    #[serde(default = "default_version")]
    pub version: u32,
    pub origin: String,
    #[serde(default)]
    pub request_url: String,
    pub chain_id: u64,
    pub escrow_contract: String,
    pub currency: String,
    pub recipient: String,
    pub payer: String,
    pub authorized_signer: String,
    pub salt: String,
    pub channel_id: String,
    pub deposit: String,
    pub tick_cost: String,
    pub cumulative_amount: String,
    pub challenge_echo: String,
    /// Explicit lifecycle state.
    #[serde(default = "default_state")]
    pub state: SessionStatus,
    /// UNIX time when close was requested (0 if not requested)
    #[serde(default)]
    pub close_requested_at: u64,
    /// UNIX time when channel is ready to finalize (0 if not applicable)
    #[serde(default)]
    pub grace_ready_at: u64,
    pub created_at: u64,
    pub last_used_at: u64,
}

fn default_version() -> u32 {
    1
}

fn default_state() -> SessionStatus {
    SessionStatus::Active
}

pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

impl SessionRecord {
    /// Parse the cumulative amount.
    pub fn cumulative_amount_u128(&self) -> anyhow::Result<u128> {
        self.cumulative_amount
            .parse()
            .context("Invalid cumulative_amount in session record")
    }

    /// Parse the deposit amount.
    pub fn deposit_u128(&self) -> anyhow::Result<u128> {
        self.deposit
            .parse()
            .context("Invalid deposit in session record")
    }

    /// Parse the channel ID.
    pub fn channel_id_b256(&self) -> anyhow::Result<alloy::primitives::B256> {
        self.channel_id
            .parse()
            .context("Invalid channel_id in session record")
    }

    /// Update the cumulative amount (monotonic: never decreases).
    pub fn set_cumulative_amount(&mut self, amount: u128) {
        let current = self.cumulative_amount.parse::<u128>().unwrap_or(0);
        self.cumulative_amount = amount.max(current).to_string();
    }

    /// Update `last_used_at` timestamp.
    pub fn touch(&mut self) {
        self.last_used_at = now_secs();
    }

    /// Derive the network from `chain_id`.
    pub fn network_id(&self) -> NetworkId {
        NetworkId::from_chain_id(self.chain_id).unwrap_or_default()
    }

    /// Compute the display status and optional remaining seconds from the session state.
    ///
    /// Returns `(status, remaining_secs)`:
    /// - Active sessions: `(SessionStatus::Active, None)`
    /// - Closing with time remaining: `(SessionStatus::Closing, Some(secs))`
    /// - Closing with grace elapsed: `(SessionStatus::Finalizable, Some(0))`
    pub fn status_at(&self, now: u64) -> (SessionStatus, Option<u64>) {
        match self.state {
            SessionStatus::Closing => {
                let rem = self.grace_ready_at.saturating_sub(now);
                if rem == 0 && self.grace_ready_at > 0 {
                    (SessionStatus::Finalizable, Some(0))
                } else {
                    (SessionStatus::Closing, Some(rem))
                }
            }
            SessionStatus::Finalizable => (SessionStatus::Finalizable, Some(0)),
            SessionStatus::Finalized => (SessionStatus::Finalized, None),
            SessionStatus::Orphaned => (SessionStatus::Orphaned, None),
            SessionStatus::Active => (SessionStatus::Active, None),
        }
    }
}

/// Compute a session key from the origin URL (extract `scheme://host[:port]`).
///
/// Non-alphanumeric chars (except `-` and `.`) are replaced with `_`.
pub fn session_key(origin: &str) -> String {
    let normalized = url::Url::parse(origin)
        .map(|u| u.origin().ascii_serialization())
        .unwrap_or_else(|_| origin.to_string());

    normalized
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_record(origin: &str, salt: &str) -> SessionRecord {
        let now = now_secs();
        SessionRecord {
            version: 1,
            origin: origin.into(),
            request_url: format!("{origin}/api/v1"),
            chain_id: 4217,
            escrow_contract: "0x00".into(),
            currency: "0x00".into(),
            recipient: "0x00".into(),
            payer: "0x00".into(),
            authorized_signer: "0x00".into(),
            salt: salt.into(),
            channel_id: "0x00".into(),
            deposit: "1000000".into(),
            tick_cost: "100".into(),
            cumulative_amount: "0".into(),
            challenge_echo: "echo".into(),
            state: SessionStatus::Active,
            close_requested_at: 0,
            grace_ready_at: 0,
            created_at: now,
            last_used_at: now,
        }
    }

    #[test]
    fn test_touch_updates_last_used() {
        let mut record = test_record("https://example.com", "salt");
        record.last_used_at = 1000;
        record.touch();
        assert!(record.last_used_at > 1000);
    }

    #[test]
    fn test_status_at_active() {
        let record = test_record("https://example.com", "salt");
        let (status, rem) = record.status_at(1000);
        assert_eq!(status, SessionStatus::Active);
        assert!(rem.is_none());
    }

    #[test]
    fn test_status_at_closing_with_remaining() {
        let mut record = test_record("https://example.com", "salt");
        record.state = SessionStatus::Closing;
        record.grace_ready_at = 2000;
        let (status, rem) = record.status_at(1500);
        assert_eq!(status, SessionStatus::Closing);
        assert_eq!(rem, Some(500));
    }

    #[test]
    fn test_status_at_closing_grace_elapsed() {
        let mut record = test_record("https://example.com", "salt");
        record.state = SessionStatus::Closing;
        record.grace_ready_at = 1000;
        let (status, rem) = record.status_at(2000);
        assert_eq!(status, SessionStatus::Finalizable);
        assert_eq!(rem, Some(0));
    }

    #[test]
    fn test_status_at_finalizable() {
        let mut record = test_record("https://example.com", "salt");
        record.state = SessionStatus::Finalizable;
        let (status, rem) = record.status_at(5000);
        assert_eq!(status, SessionStatus::Finalizable);
        assert_eq!(rem, Some(0));
    }

    #[test]
    fn test_status_at_finalized() {
        let mut record = test_record("https://example.com", "salt");
        record.state = SessionStatus::Finalized;
        let (status, rem) = record.status_at(1000);
        assert_eq!(status, SessionStatus::Finalized);
        assert!(rem.is_none());
    }

    #[test]
    fn test_status_at_orphaned() {
        let mut record = test_record("https://example.com", "salt");
        record.state = SessionStatus::Orphaned;
        let (status, rem) = record.status_at(1000);
        assert_eq!(status, SessionStatus::Orphaned);
        assert!(rem.is_none());
    }

    #[test]
    fn test_network_id_tempo() {
        let record = test_record("https://example.com", "salt");
        assert_eq!(record.chain_id, 4217);
        assert_eq!(record.network_id(), NetworkId::Tempo);
    }

    #[test]
    fn test_network_id_moderato() {
        let mut record = test_record("https://example.com", "salt");
        record.chain_id = 42431;
        assert_eq!(record.network_id(), NetworkId::TempoModerato);
    }

    #[test]
    fn test_cumulative_amount_u128_valid() {
        let mut record = test_record("https://example.com", "salt");
        record.cumulative_amount = "1000".into();
        assert_eq!(record.cumulative_amount_u128().unwrap(), 1000u128);
    }

    #[test]
    fn test_cumulative_amount_u128_invalid() {
        let mut record = test_record("https://example.com", "salt");
        record.cumulative_amount = "abc".into();
        assert!(record.cumulative_amount_u128().is_err());
    }

    #[test]
    fn test_deposit_u128_valid() {
        let mut record = test_record("https://example.com", "salt");
        record.deposit = "5000000".into();
        assert_eq!(record.deposit_u128().unwrap(), 5000000u128);
    }

    #[test]
    fn test_deposit_u128_invalid() {
        let mut record = test_record("https://example.com", "salt");
        record.deposit = "".into();
        assert!(record.deposit_u128().is_err());
    }

    #[test]
    fn test_channel_id_b256_valid() {
        let mut record = test_record("https://example.com", "salt");
        record.channel_id =
            "0x0000000000000000000000000000000000000000000000000000000000000001".into();
        let b = record.channel_id_b256().unwrap();
        assert_eq!(
            b,
            alloy::primitives::B256::from(alloy::primitives::U256::from(1))
        );
    }

    #[test]
    fn test_channel_id_b256_invalid() {
        let mut record = test_record("https://example.com", "salt");
        record.channel_id = "not_hex".into();
        assert!(record.channel_id_b256().is_err());
    }

    #[test]
    fn test_session_status_round_trip() {
        let variants = [
            SessionStatus::Active,
            SessionStatus::Closing,
            SessionStatus::Finalizable,
            SessionStatus::Finalized,
            SessionStatus::Orphaned,
        ];
        for variant in variants {
            let s = variant.as_str();
            let parsed = SessionStatus::from_db_str(s);
            assert_eq!(parsed, variant, "round-trip failed for {s}");
        }
    }

    #[test]
    fn test_session_status_unknown_defaults_to_active() {
        assert_eq!(SessionStatus::from_db_str("garbage"), SessionStatus::Active);
        assert_eq!(SessionStatus::from_db_str(""), SessionStatus::Active);
    }

    #[test]
    fn test_session_key_basic() {
        assert_eq!(
            session_key("https://api.example.com/v1/chat"),
            "https___api.example.com"
        );
    }

    #[test]
    fn test_session_key_with_port() {
        assert_eq!(
            session_key("http://localhost:8080/foo"),
            "http___localhost_8080"
        );
    }

    #[test]
    fn test_session_key_no_path() {
        assert_eq!(session_key("https://example.com"), "https___example.com");
    }

    #[test]
    fn test_session_key_different_paths_same_origin() {
        assert_eq!(
            session_key("https://example.com/v1/chat"),
            session_key("https://example.com/v2/other")
        );
        assert_eq!(
            session_key("https://example.com/a?foo=bar"),
            session_key("https://example.com/b#frag")
        );
    }

    #[test]
    fn test_session_key_trailing_slash() {
        assert_eq!(
            session_key("https://example.com/"),
            session_key("https://example.com")
        );
    }

    #[test]
    fn test_session_key_query_params_stripped() {
        assert_eq!(
            session_key("https://example.com/path?foo=bar"),
            session_key("https://example.com/other")
        );
    }
}
