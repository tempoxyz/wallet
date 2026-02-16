//! Persistent session storage for payment channels across CLI invocations.
//!
//! Sessions are stored as individual TOML files in the data directory,
//! keyed by the origin (scheme://host[:port]) of the endpoint.

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::error::PrestoError;

/// Session TTL: 24 hours.
pub const SESSION_TTL_SECS: u64 = 24 * 60 * 60;

/// A persisted payment channel session.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SessionRecord {
    #[serde(default = "default_version")]
    pub version: u32,
    pub origin: String,
    pub network_name: String,
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
    pub did: String,
    pub challenge_echo: String,
    pub challenge_id: String,
    pub created_at: u64,
    pub last_used_at: u64,
    pub expires_at: u64,
}

fn default_version() -> u32 {
    1
}

impl SessionRecord {
    /// Returns `true` if this session has expired.
    pub fn is_expired(&self) -> bool {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        now > self.expires_at
    }

    /// Update `last_used_at` and extend `expires_at`.
    pub fn touch(&mut self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.last_used_at = now;
        self.expires_at = now + SESSION_TTL_SECS;
    }
}

/// Get the sessions directory, creating it if needed.
pub fn sessions_dir() -> Result<PathBuf> {
    let dir = dirs::data_dir()
        .ok_or(PrestoError::NoConfigDir)?
        .join("presto")
        .join("sessions");
    fs::create_dir_all(&dir).context("Failed to create sessions directory")?;
    Ok(dir)
}

/// Compute a session key from the origin URL (extract `scheme://host[:port]`).
///
/// Non-alphanumeric chars (except `-` and `.`) are replaced with `_`.
pub fn session_key(origin: &str) -> String {
    let normalized = match url::Url::parse(origin) {
        Ok(parsed) => {
            let scheme = parsed.scheme();
            let host = parsed.host_str().unwrap_or("unknown");
            match parsed.port() {
                Some(port) => format!("{scheme}://{host}:{port}"),
                None => format!("{scheme}://{host}"),
            }
        }
        Err(_) => origin.to_string(),
    };

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

/// Load a session record by key. Returns `None` if not found.
pub fn load_session(key: &str) -> Result<Option<SessionRecord>> {
    let path = sessions_dir()?.join(format!("{key}.toml"));
    match fs::read_to_string(&path) {
        Ok(contents) => {
            let record: SessionRecord =
                toml::from_str(&contents).context("Failed to parse session file")?;
            Ok(Some(record))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).context("Failed to read session file"),
    }
}

/// Save a session record to disk.
pub fn save_session(record: &SessionRecord) -> Result<()> {
    let key = session_key(&record.origin);
    let path = sessions_dir()?.join(format!("{key}.toml"));
    let contents = toml::to_string_pretty(record).context("Failed to serialize session")?;
    fs::write(&path, contents).context("Failed to write session file")?;
    Ok(())
}

/// Delete a session record by key.
pub fn delete_session(key: &str) -> Result<()> {
    let path = sessions_dir()?.join(format!("{key}.toml"));
    match fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).context("Failed to delete session file"),
    }
}

/// List all session records.
pub fn list_sessions() -> Result<Vec<SessionRecord>> {
    let dir = sessions_dir()?;
    let mut records = Vec::new();
    for entry in fs::read_dir(&dir).context("Failed to read sessions directory")? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "toml") {
            match fs::read_to_string(&path) {
                Ok(contents) => {
                    if let Ok(record) = toml::from_str::<SessionRecord>(&contents) {
                        records.push(record);
                    }
                }
                Err(_) => continue,
            }
        }
    }
    Ok(records)
}

/// Delete expired sessions and return them (for best-effort close).
#[allow(dead_code)]
pub fn collect_expired_sessions() -> Result<Vec<SessionRecord>> {
    let sessions = list_sessions()?;
    let mut expired = Vec::new();
    for session in sessions {
        if session.is_expired() {
            let key = session_key(&session.origin);
            delete_session(&key)?;
            expired.push(session);
        }
    }
    Ok(expired)
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_is_expired_future() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let record = SessionRecord {
            version: 1,
            origin: "https://example.com".into(),
            network_name: "tempo".into(),
            chain_id: 4217,
            escrow_contract: "0x00".into(),
            currency: "0x00".into(),
            recipient: "0x00".into(),
            payer: "0x00".into(),
            authorized_signer: "0x00".into(),
            salt: "0x00".into(),
            channel_id: "0x00".into(),
            deposit: "1000000".into(),
            tick_cost: "100".into(),
            cumulative_amount: "0".into(),
            did: "did:pkh:eip155:4217:0x00".into(),
            challenge_echo: "echo".into(),
            challenge_id: "id".into(),
            created_at: now,
            last_used_at: now,
            expires_at: now + SESSION_TTL_SECS,
        };
        assert!(!record.is_expired());
    }

    #[test]
    fn test_is_expired_past() {
        let record = SessionRecord {
            version: 1,
            origin: "https://example.com".into(),
            network_name: "tempo".into(),
            chain_id: 4217,
            escrow_contract: "0x00".into(),
            currency: "0x00".into(),
            recipient: "0x00".into(),
            payer: "0x00".into(),
            authorized_signer: "0x00".into(),
            salt: "0x00".into(),
            channel_id: "0x00".into(),
            deposit: "1000000".into(),
            tick_cost: "100".into(),
            cumulative_amount: "0".into(),
            did: "did:pkh:eip155:4217:0x00".into(),
            challenge_echo: "echo".into(),
            challenge_id: "id".into(),
            created_at: 1000,
            last_used_at: 1000,
            expires_at: 1000,
        };
        assert!(record.is_expired());
    }
}
