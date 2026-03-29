use std::path::{Path, PathBuf};

use walkdir::WalkDir;

use crate::error::AppError;

pub fn scan_directory(dir: &Path) -> Result<Vec<PathBuf>, AppError> {
    let supported = ["mp3", "flac"];
    let files = WalkDir::new(dir)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| supported.contains(&ext.to_lowercase().as_str()))
                .unwrap_or(false)
        })
        .map(|e| e.path().to_owned())
        .collect();
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_nonexistent_dir_returns_empty() {
        let result = scan_directory(Path::new("/nonexistent/path/xyz"));
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn scan_current_dir_does_not_panic() {
        let result = scan_directory(Path::new("."));
        assert!(result.is_ok());
    }
}
