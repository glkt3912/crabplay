use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Playlist {
    pub name: String,
    pub paths: Vec<PathBuf>,
}

impl Playlist {
    pub fn new(name: impl Into<String>, paths: Vec<PathBuf>) -> Self {
        Self {
            name: name.into(),
            paths,
        }
    }

    /// `dir` 配下に `<name>.json` として保存し、保存先パスを返す。
    pub fn save(&self, dir: &Path) -> Result<PathBuf> {
        std::fs::create_dir_all(dir)?;
        let trimmed = self.name.trim();
        anyhow::ensure!(
            !trimmed.is_empty(),
            "playlist name must not be empty or whitespace-only"
        );
        let safe: String = trimmed
            .chars()
            .map(|c| {
                if c == '/' || c == '\0' || c.is_control() {
                    '_'
                } else {
                    c
                }
            })
            .collect();
        let dest = dir.join(format!("{safe}.json"));
        std::fs::write(&dest, serde_json::to_string_pretty(self)?)?;
        Ok(dest)
    }

    /// JSON ファイルからプレイリストを読み込む。
    pub fn load(path: &Path) -> Result<Self> {
        let data = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&data)?)
    }

    /// デフォルト保存先。XDG_CONFIG_HOME → HOME/.config → カレントディレクトリの順でフォールバック。
    pub fn default_dir() -> PathBuf {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .unwrap_or_else(|| PathBuf::from("."));
        base.join("crabplay").join("playlists")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_preserves_name_and_paths() {
        let dir = tempfile::tempdir().unwrap();
        let paths = vec![
            PathBuf::from("/music/track1.mp3"),
            PathBuf::from("/music/track2.flac"),
        ];
        let original = Playlist::new("my-playlist", paths.clone());
        let saved = original.save(dir.path()).unwrap();

        let loaded = Playlist::load(&saved).unwrap();
        assert_eq!(loaded.name, "my-playlist");
        assert_eq!(loaded.paths, paths);
    }

    #[test]
    fn roundtrip_non_ascii_name() {
        let dir = tempfile::tempdir().unwrap();
        let original = Playlist::new("お気に入り", vec![PathBuf::from("/music/a.mp3")]);
        let saved = original.save(dir.path()).unwrap();

        assert_eq!(saved.file_name().unwrap(), "お気に入り.json");
        let loaded = Playlist::load(&saved).unwrap();
        assert_eq!(loaded.name, "お気に入り");
    }

    #[test]
    fn save_trims_whitespace_from_filename() {
        let dir = tempfile::tempdir().unwrap();
        let pl = Playlist::new("  spaced  ", vec![]);
        let saved = pl.save(dir.path()).unwrap();

        assert_eq!(saved.file_name().unwrap(), "spaced.json");
        let loaded = Playlist::load(&saved).unwrap();
        assert_eq!(loaded.name, "  spaced  ");
    }

    #[test]
    fn save_replaces_slash_with_underscore() {
        let dir = tempfile::tempdir().unwrap();
        let pl = Playlist::new("BGM/作業用", vec![]);
        let saved = pl.save(dir.path()).unwrap();

        assert_eq!(saved.file_name().unwrap(), "BGM_作業用.json");
    }

    #[test]
    fn save_replaces_control_chars() {
        let dir = tempfile::tempdir().unwrap();
        let pl = Playlist::new("name\twith\ttabs", vec![]);
        let saved = pl.save(dir.path()).unwrap();

        assert_eq!(saved.file_name().unwrap(), "name_with_tabs.json");
    }

    #[test]
    fn save_rejects_empty_name() {
        let dir = tempfile::tempdir().unwrap();
        let pl = Playlist::new("", vec![]);
        assert!(pl.save(dir.path()).is_err());
    }

    #[test]
    fn save_rejects_whitespace_only_name() {
        let dir = tempfile::tempdir().unwrap();
        let pl = Playlist::new("   ", vec![]);
        assert!(pl.save(dir.path()).is_err());
    }

    #[test]
    fn load_nonexistent_file_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let result = Playlist::load(&dir.path().join("does_not_exist.json"));
        assert!(result.is_err());
    }

    #[test]
    fn save_replaces_null_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let pl = Playlist::new("name\0with\0nulls", vec![]);
        let saved = pl.save(dir.path()).unwrap();

        assert_eq!(saved.file_name().unwrap(), "name_with_nulls.json");
    }

    #[test]
    fn roundtrip_with_nonexistent_track_paths() {
        let dir = tempfile::tempdir().unwrap();
        let paths = vec![PathBuf::from("/does/not/exist.mp3")];
        let original = Playlist::new("ghost-tracks", paths.clone());
        let saved = original.save(dir.path()).unwrap();

        let loaded = Playlist::load(&saved).unwrap();
        assert_eq!(loaded.paths, paths);
    }
}
