//! Per-origin file locking for session operations.

use std::{error::Error, fs::OpenOptions, path::Path};

use crate::error::{PaymentError, TempoError};

use super::storage::ensure_wallet_dir;

fn lock_error<E>(operation: &'static str, source: E) -> TempoError
where
    E: Error + Send + Sync + 'static,
{
    PaymentError::ChannelPersistenceSource {
        operation,
        source: Box::new(source),
    }
    .into()
}

/// File lock guard for an origin/session key.
pub struct ChannelLock {
    file: std::fs::File,
}

impl Drop for ChannelLock {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.file);
    }
}

/// Acquire a per-origin exclusive lock to serialize open/persist operations.
///
/// Uses a **blocking** lock so that concurrent workers wait for the first
/// to finish rather than proceeding unlocked. The caller should drop the
/// returned guard when the request-level critical section is complete.
/// The session request flow intentionally holds this lock across the paid
/// request lifecycle (including voucher replay/streaming) so same-origin
/// requests cannot race cumulative voucher updates on a shared channel.
///
/// # Errors
///
/// Returns an error when the wallet directory cannot be created, lock file
/// creation/open fails, or the file lock cannot be acquired.
pub fn acquire_origin_lock(key: &str) -> Result<ChannelLock, TempoError> {
    let dir = ensure_wallet_dir()?;
    acquire_origin_lock_in_dir(key, &dir)
}

fn acquire_origin_lock_in_dir(key: &str, dir: &Path) -> Result<ChannelLock, TempoError> {
    let lock_path = dir.join(format!("{key}.lock"));
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(lock_path)
        .map_err(|err| lock_error("open session lock file", err))?;
    fs2::FileExt::lock_exclusive(&file).map_err(|err| lock_error("acquire session lock", err))?;
    Ok(ChannelLock { file })
}

#[cfg(test)]
mod tests {
    use super::{super::model::session_key, *};

    #[test]
    fn test_origin_lock_is_exclusive() {
        // Use an explicit temp directory to avoid process-global env races.
        let tmp = tempfile::tempdir().unwrap();
        let lock_dir = tmp.path();

        let key = session_key("https://example.com");
        let lock1 = acquire_origin_lock_in_dir(&key, lock_dir).expect("first lock should succeed");

        // Verify exclusivity from another thread: try_lock should fail
        // while the first guard is held.
        let key_clone = key.clone();
        let lock_dir = lock_dir.to_path_buf();
        let lock_dir_for_thread = lock_dir.clone();
        let result = std::thread::spawn(move || {
            let lock_path = lock_dir_for_thread.join(format!("{key_clone}.lock"));
            let file = OpenOptions::new()
                .create(true)
                .truncate(false)
                .read(true)
                .write(true)
                .open(lock_path)
                .unwrap();
            fs2::FileExt::try_lock_exclusive(&file)
        })
        .join()
        .unwrap();
        assert!(
            result.is_err(),
            "second lock from another thread should fail while first is held"
        );

        drop(lock1);

        // After drop, we should be able to re-acquire
        acquire_origin_lock_in_dir(&key, &lock_dir).expect("re-acquire after drop should succeed");
    }
}
