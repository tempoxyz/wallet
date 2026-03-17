use std::{
    error::Error,
    fs::OpenOptions,
    path::{Path, PathBuf},
};

use tempo_common::{
    error::{PaymentError, TempoError},
    tempo_home,
};

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

pub(super) struct OriginLock {
    file: std::fs::File,
}

impl Drop for OriginLock {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.file);
    }
}

fn wallet_dir() -> Result<PathBuf, TempoError> {
    let dir = tempo_home()?.join("wallet");
    std::fs::create_dir_all(&dir).map_err(|source| lock_error("ensure charge lock dir", source))?;
    Ok(dir)
}

pub(super) fn origin_lock_key(url_or_origin: &str) -> String {
    let normalized = url::Url::parse(url_or_origin).map_or_else(
        |_| url_or_origin.to_string(),
        |u| u.origin().ascii_serialization(),
    );
    let safe: String = normalized
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("payment_{safe}")
}

fn acquire_lock_in_dir(key: &str, dir: &Path) -> Result<OriginLock, TempoError> {
    let path = dir.join(format!("{key}.lock"));
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|source| lock_error("open charge lock file", source))?;
    fs2::FileExt::lock_exclusive(&file)
        .map_err(|source| lock_error("acquire charge lock", source))?;
    Ok(OriginLock { file })
}

pub(super) fn acquire_origin_lock(key: &str) -> Result<OriginLock, TempoError> {
    let dir = wallet_dir()?;
    acquire_lock_in_dir(key, &dir)
}
