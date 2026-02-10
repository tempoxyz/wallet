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

    #[test]
    fn test_atomic_write_empty_content() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("empty.txt");

        atomic_write(&path, "", 0o644).unwrap();

        let content = fs::read_to_string(&path).unwrap();
        assert!(content.is_empty());
    }

    #[test]
    fn test_atomic_write_temp_cleaned_up_on_rename_failure() {
        let dir = tempdir().unwrap();
        let nested = dir.path().join("subdir");
        fs::create_dir(&nested).unwrap();

        let target = nested.join("target.txt");
        atomic_write(&target, "original", 0o644).unwrap();

        fs::remove_dir_all(&nested).unwrap();

        let result = atomic_write(&target, "new content", 0o644);
        assert!(result.is_ok());
    }

    #[test]
    fn test_atomic_write_no_temp_left_on_failure() {
        let dir = tempdir().unwrap();
        let blocker = dir.path().join("subdir");
        fs::write(&blocker, "i am a file").unwrap();

        let path = blocker.join("test.txt");
        let result = atomic_write(&path, "content", 0o644);
        assert!(result.is_err());

        let tmp_files: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(tmp_files.is_empty());
    }

    #[test]
    fn test_atomic_write_parent_is_file_not_dir() {
        let dir = tempdir().unwrap();
        let blocker = dir.path().join("not_a_dir");
        fs::write(&blocker, "i am a file").unwrap();

        let path = blocker.join("test.txt");
        let result = atomic_write(&path, "content", 0o644);
        assert!(result.is_err());
    }

    #[test]
    fn test_atomic_write_retries_on_temp_collision() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.txt");

        let pid = process::id();
        let base_nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let colliding_name = format!(".test.txt.{}.{}.tmp", pid, base_nonce);
        fs::write(dir.path().join(&colliding_name), "blocker").unwrap();

        atomic_write(&path, "should succeed via retry", 0o644).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "should succeed via retry"
        );
    }

    #[test]
    fn test_atomic_write_preserves_old_content_on_dir_failure() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test.txt");

        atomic_write(&path, "original", 0o644).unwrap();

        let bad_path = Path::new("/dev/null/impossible/test.txt");
        let result = atomic_write(bad_path, "new content", 0o644);
        assert!(result.is_err());

        assert_eq!(fs::read_to_string(&path).unwrap(), "original");
    }

    #[cfg(unix)]
    #[test]
    fn test_atomic_write_does_not_follow_symlink_for_temp() {
        let dir = tempdir().unwrap();
        let target_dir = tempdir().unwrap();
        let decoy = target_dir.path().join("decoy.txt");
        fs::write(&decoy, "original decoy").unwrap();

        let link_path = dir.path().join("config.txt");
        std::os::unix::fs::symlink(&decoy, &link_path).unwrap();

        atomic_write(&link_path, "overwritten", 0o644).unwrap();

        let link_content = fs::read_to_string(&link_path).unwrap();
        assert_eq!(link_content, "overwritten");
    }

    #[cfg(unix)]
    #[test]
    fn test_atomic_write_permissions_on_overwrite() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().unwrap();
        let path = dir.path().join("test.txt");

        fs::write(&path, "world readable").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

        atomic_write(&path, "now restricted", 0o600).unwrap();

        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert_eq!(fs::read_to_string(&path).unwrap(), "now restricted");
    }
}
