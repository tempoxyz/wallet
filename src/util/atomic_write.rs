use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use crate::error::{Result, TempoCtlError};

struct TempFileGuard {
    path: Option<PathBuf>,
}

impl TempFileGuard {
    fn new(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    fn defuse(&mut self) {
        self.path = None;
    }
}

impl Drop for TempFileGuard {
    fn drop(&mut self) {
        if let Some(ref path) = self.path {
            let _ = fs::remove_file(path);
        }
    }
}

pub fn atomic_write(
    path: &Path,
    contents: &str,
    #[allow(unused_variables)] unix_mode: u32,
) -> Result<()> {
    let parent = path.parent().ok_or_else(|| {
        TempoCtlError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("path has no parent directory: {}", path.display()),
        ))
    })?;

    fs::create_dir_all(parent)?;

    if !parent.is_dir() {
        return Err(TempoCtlError::Io(std::io::Error::new(
            std::io::ErrorKind::NotADirectory,
            format!("parent is not a directory: {}", parent.display()),
        )));
    }

    let filename = path
        .file_name()
        .ok_or_else(|| {
            TempoCtlError::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("path has no filename: {}", path.display()),
            ))
        })?
        .to_string_lossy();

    let pid = process::id();
    let base_nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();

    let mut last_err = None;
    for attempt in 0..10u128 {
        let nonce = base_nonce ^ attempt;
        let tmp_name = format!(".{}.{}.{}.tmp", filename, pid, nonce);
        let tmp_path = parent.join(&tmp_name);

        let mut opts = OpenOptions::new();
        opts.write(true).create_new(true);

        #[cfg(unix)]
        opts.mode(unix_mode);

        match opts.open(&tmp_path) {
            Ok(file) => {
                let mut guard = TempFileGuard::new(tmp_path.clone());
                write_and_rename(file, contents, &tmp_path, path)?;
                guard.defuse();
                return Ok(());
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                last_err = Some(e);
                continue;
            }
            Err(e) => return Err(e.into()),
        }
    }

    Err(last_err.map(TempoCtlError::Io).unwrap_or_else(|| {
        TempoCtlError::Io(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "failed to create temp file after 10 attempts",
        ))
    }))
}

fn write_and_rename(
    mut file: File,
    contents: &str,
    tmp_path: &Path,
    final_path: &Path,
) -> Result<()> {
    file.write_all(contents.as_bytes())?;
    file.sync_all()?;
    fs::rename(tmp_path, final_path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::tempdir;

    #[test]
    fn test_atomic_write_creates_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.txt");

        atomic_write(&path, "hello world", 0o644).unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "hello world");
    }

    #[test]
    fn test_atomic_write_creates_parent_dirs() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("a").join("b").join("c").join("test.txt");

        atomic_write(&path, "nested content", 0o644).unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "nested content");
    }

    #[test]
    fn test_atomic_write_overwrites_existing() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.txt");

        atomic_write(&path, "first", 0o644).unwrap();
        atomic_write(&path, "second", 0o644).unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "second");
    }

    #[test]
    fn test_atomic_write_no_temp_left_on_success() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.txt");

        atomic_write(&path, "content", 0o644).unwrap();

        let tmp_files: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();

        assert!(tmp_files.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn test_atomic_write_sets_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let path = dir.path().join("test.txt");

        atomic_write(&path, "secret", 0o600).unwrap();

        let metadata = fs::metadata(&path).unwrap();
        let mode = metadata.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}
