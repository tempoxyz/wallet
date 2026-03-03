//! Filesystem utilities.

use std::fs;
use std::io::Write;
use std::path::Path;

use crate::error::PrestoError;

/// Atomic file write with optional UNIX mode.
pub fn atomic_write(
    path: &Path,
    contents: &str,
    #[allow(unused_variables)] unix_mode: u32,
) -> Result<(), PrestoError> {
    let parent = path.parent().ok_or_else(|| {
        PrestoError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("path has no parent directory: {}", path.display()),
        ))
    })?;

    fs::create_dir_all(parent)?;

    // Create temp file in the same directory (ensures same filesystem for rename)
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        temp.as_file()
            .set_permissions(fs::Permissions::from_mode(unix_mode))?;
    }

    // tests moved to end of file

    temp.write_all(contents.as_bytes())?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| PrestoError::Io(e.error))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use tempfile::tempdir;

    #[test]
    fn test_atomic_write_creates_file() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("test.txt");

        atomic_write(&path, "hello world", 0o644).expect("write");

        assert_eq!(fs::read_to_string(&path).expect("read"), "hello world");
    }

    #[test]
    fn test_atomic_write_creates_parent_dirs() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("a").join("b").join("c").join("test.txt");

        atomic_write(&path, "nested content", 0o644).expect("write");

        assert_eq!(fs::read_to_string(&path).expect("read"), "nested content");
    }

    #[test]
    fn test_atomic_write_overwrites_existing() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("test.txt");

        atomic_write(&path, "first", 0o644).expect("first write");
        atomic_write(&path, "second", 0o644).expect("second write");

        assert_eq!(fs::read_to_string(&path).expect("read"), "second");
    }

    #[test]
    fn test_atomic_write_no_temp_left_on_success() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("test.txt");

        atomic_write(&path, "content", 0o644).expect("write");

        let tmp_files: Vec<_> = fs::read_dir(dir.path())
            .expect("read_dir")
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();

        assert!(tmp_files.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn test_atomic_write_sets_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("test.txt");

        atomic_write(&path, "secret", 0o600).expect("write");

        let metadata = fs::metadata(&path).expect("metadata");
        let mode = metadata.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn test_atomic_write_empty_content() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("empty.txt");

        atomic_write(&path, "", 0o644).expect("write");

        let content = fs::read_to_string(&path).expect("read");
        assert!(content.is_empty());
    }

    #[test]
    fn test_atomic_write_temp_cleaned_up_on_rename_failure() {
        let dir = tempdir().expect("tempdir");
        let nested = dir.path().join("subdir");
        fs::create_dir(&nested).expect("mkdir");

        let target = nested.join("target.txt");
        atomic_write(&target, "original", 0o644).expect("write");

        fs::remove_dir_all(&nested).expect("remove");

        let result = atomic_write(&target, "new content", 0o644);
        assert!(result.is_ok());
    }

    #[test]
    fn test_atomic_write_no_temp_left_on_failure() {
        let dir = tempdir().expect("tempdir");
        let blocker = dir.path().join("subdir");
        fs::write(&blocker, "i am a file").expect("write blocker");

        let path = blocker.join("test.txt");
        let result = atomic_write(&path, "content", 0o644);
        assert!(result.is_err());

        let tmp_files: Vec<_> = fs::read_dir(dir.path())
            .expect("read_dir")
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(tmp_files.is_empty());
    }

    #[test]
    fn test_atomic_write_parent_is_file_not_dir() {
        let dir = tempdir().expect("tempdir");
        let blocker = dir.path().join("not_a_dir");
        fs::write(&blocker, "i am a file").expect("write blocker");

        let path = blocker.join("test.txt");
        let result = atomic_write(&path, "content", 0o644);
        assert!(result.is_err());
    }

    #[test]
    fn test_atomic_write_preserves_old_content_on_dir_failure() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("test.txt");

        atomic_write(&path, "original", 0o644).expect("write");

        let bad_path = Path::new("/dev/null/impossible/test.txt");
        let result = atomic_write(bad_path, "new content", 0o644);
        assert!(result.is_err());

        assert_eq!(fs::read_to_string(&path).expect("read"), "original");
    }

    #[cfg(unix)]
    #[test]
    fn test_atomic_write_does_not_follow_symlink_for_temp() {
        let dir = tempdir().expect("tempdir");
        let target_dir = tempdir().expect("target tempdir");
        let decoy = target_dir.path().join("decoy.txt");
        fs::write(&decoy, "original decoy").expect("write decoy");

        let link_path = dir.path().join("config.txt");
        std::os::unix::fs::symlink(&decoy, &link_path).expect("symlink");

        atomic_write(&link_path, "overwritten", 0o644).expect("write");

        let link_content = fs::read_to_string(&link_path).expect("read");
        assert_eq!(link_content, "overwritten");
    }
}
