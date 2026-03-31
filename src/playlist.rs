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
        let safe: String = self
            .name
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
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

    /// デフォルト保存先: `~/.config/crabplay/playlists/`
    pub fn default_dir() -> PathBuf {
        std::env::var("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(".config")
            .join("crabplay")
            .join("playlists")
    }
}
