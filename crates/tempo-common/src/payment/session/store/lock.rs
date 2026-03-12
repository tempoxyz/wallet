//! Per-origin file locking for session operations.

use std::fs::OpenOptions;

use anyhow::{Context, Result};

use super::storage::ensure_wallet_dir;

/// File lock guard for an origin/session key.
pub struct SessionLock {
    file: std::fs::File,
}

impl Drop for SessionLock {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.file);
    }
}

/// Acquire a per-origin exclusive lock to serialize open/persist operations.
pub fn acquire_origin_lock(key: &str) -> Result<SessionLock> {
    let dir = ensure_wallet_dir()?;
    let lock_path = dir.join(format!("{}.lock", key));
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)
        .context("Failed to create/open session lock file")?;
    fs2::FileExt::try_lock_exclusive(&file).context("Failed to acquire session lock")?;
    Ok(SessionLock { file })
}

#[cfg(test)]
mod tests {
    use super::super::model::session_key;
    use super::*;

    #[test]
    fn test_origin_lock_is_exclusive() {
        // Redirect HOME to a temp directory to isolate lock files
        let tmp = tempfile::tempdir().unwrap();
        std::env::set_var("HOME", tmp.path());

        let key = session_key("https://example.com");
        let lock1 = acquire_origin_lock(&key).expect("first lock should succeed");

        // Second lock should fail while the first guard is held
        let second = acquire_origin_lock(&key);
        assert!(second.is_err(), "second lock should be exclusive-error");

        drop(lock1);

        // After drop, we should be able to re-acquire
        acquire_origin_lock(&key).expect("re-acquire after drop should succeed");
    }
}
