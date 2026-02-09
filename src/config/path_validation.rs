//! Path validation utilities to prevent directory traversal attacks.

use std::path::{Component, PathBuf};

use crate::error::TempoCtlError;

/// Validates that a path doesn't contain directory traversal sequences.
/// Returns the validated path or an error if traversal is detected.
pub fn validate_path(path: &str, allow_absolute: bool) -> Result<PathBuf, TempoCtlError> {
    let path = PathBuf::from(path);

    // Check for parent directory components (..)
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(TempoCtlError::ConfigMissing(
            "Path traversal (..) not allowed".to_string(),
        ));
    }

    // Optionally reject absolute paths
    if !allow_absolute && path.is_absolute() {
        return Err(TempoCtlError::ConfigMissing(
            "Absolute paths not allowed for this option".to_string(),
        ));
    }

    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_relative_path() {
        let result = validate_path("output.txt", false);
        assert!(result.is_ok());
        assert_eq!(
            result.expect("Valid path should be returned"),
            PathBuf::from("output.txt")
        );
    }

    #[test]
    fn test_valid_nested_relative_path() {
        let result = validate_path("dir/subdir/file.txt", false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_path_traversal_rejected() {
        let result = validate_path("../etc/passwd", false);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Path traversal"));
    }

    #[test]
    fn test_nested_path_traversal_rejected() {
        let result = validate_path("foo/../bar/../../etc/passwd", false);
        assert!(result.is_err());
    }

    #[test]
    fn test_absolute_path_rejected_when_not_allowed() {
        let result = validate_path("/etc/passwd", false);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Absolute paths not allowed"));
    }

    #[test]
    fn test_absolute_path_allowed_when_specified() {
        let result = validate_path("/home/user/config.toml", true);
        assert!(result.is_ok());
    }

    #[test]
    fn test_absolute_path_with_traversal_rejected() {
        let result = validate_path("/home/user/../etc/passwd", true);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Path traversal"));
    }

    #[test]
    fn test_current_dir_allowed() {
        let result = validate_path("./file.txt", false);
        assert!(result.is_ok());
    }
}
