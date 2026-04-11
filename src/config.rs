use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::app::RepeatMode;

/// 最大保持件数。
const MAX_RECENT: usize = 10;

/// `XDG_CONFIG_HOME` → `HOME/.config` → `.` の優先順で XDG 設定ベースディレクトリを返す。
pub fn xdg_config_base() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn default_volume() -> f32 {
    1.0
}

/// `~/.config/crabplay/config.toml` に保存するアプリ設定。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// 最近使用したディレクトリパス一覧（新しい順、最大 [`MAX_RECENT`] 件）。
    #[serde(default)]
    pub recent_dirs: Vec<PathBuf>,
    /// 音量（0.0〜2.0）。起動時に復元される。
    #[serde(default = "default_volume")]
    pub volume: f32,
    /// リピートモード。起動時に復元される。
    #[serde(default)]
    pub repeat: RepeatMode,
    /// シャッフル再生。起動時に復元される。
    #[serde(default)]
    pub shuffle: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            recent_dirs: Vec::new(),
            volume: default_volume(),
            repeat: RepeatMode::default(),
            shuffle: false,
        }
    }
}

impl Config {
    /// 設定ファイルを読み込む。ファイルが存在しない・パースに失敗した場合はデフォルト値を返す。
    pub fn load() -> Self {
        let path = Self::default_path();
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// 設定ファイルに書き出す。親ディレクトリが存在しない場合は自動作成する。
    pub fn save(&self) -> Result<()> {
        let path = Self::default_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, toml::to_string(self)?)?;
        Ok(())
    }

    /// `dir` を `recent_dirs` の先頭に追加する。重複は除去し、[`MAX_RECENT`] 件を超えた末尾を切り捨てる。
    pub fn push_recent_dir(&mut self, dir: PathBuf) {
        self.recent_dirs.retain(|d| d != &dir);
        self.recent_dirs.insert(0, dir);
        self.recent_dirs.truncate(MAX_RECENT);
    }

    /// `dir` を `recent_dirs` から削除する。存在しない場合は何もしない。
    pub fn remove_recent_dir(&mut self, dir: &PathBuf) {
        self.recent_dirs.retain(|d| d != dir);
    }

    /// デフォルト設定ファイルパス。
    pub fn default_path() -> PathBuf {
        xdg_config_base().join("crabplay").join("config.toml")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_recent_dir_deduplicates_and_caps() {
        let mut config = Config::default();
        for i in 0..12 {
            config.push_recent_dir(PathBuf::from(format!("/music/dir{i}")));
        }
        assert_eq!(config.recent_dirs.len(), MAX_RECENT);
        assert_eq!(config.recent_dirs[0], PathBuf::from("/music/dir11"));
    }

    #[test]
    fn push_recent_dir_moves_existing_to_front() {
        let mut config = Config::default();
        config.push_recent_dir(PathBuf::from("/a"));
        config.push_recent_dir(PathBuf::from("/b"));
        config.push_recent_dir(PathBuf::from("/a"));
        assert_eq!(
            config.recent_dirs,
            vec![PathBuf::from("/a"), PathBuf::from("/b")]
        );
    }

    #[test]
    fn remove_recent_dir_removes_existing() {
        let mut config = Config::default();
        config.push_recent_dir(PathBuf::from("/a"));
        config.push_recent_dir(PathBuf::from("/b"));
        config.push_recent_dir(PathBuf::from("/c"));
        config.remove_recent_dir(&PathBuf::from("/b"));
        assert_eq!(
            config.recent_dirs,
            vec![PathBuf::from("/c"), PathBuf::from("/a")]
        );
    }

    #[test]
    fn remove_recent_dir_noop_if_missing() {
        let mut config = Config::default();
        config.push_recent_dir(PathBuf::from("/a"));
        config.remove_recent_dir(&PathBuf::from("/nonexistent"));
        assert_eq!(config.recent_dirs, vec![PathBuf::from("/a")]);
    }

    #[test]
    fn roundtrip_save_load() {
        let dir = tempfile::tempdir().unwrap();
        // Override default path by writing directly
        let path = dir.path().join("config.toml");
        let mut config = Config::default();
        config.push_recent_dir(PathBuf::from("/music/jazz"));
        config.push_recent_dir(PathBuf::from("/music/rock"));

        std::fs::write(&path, toml::to_string(&config).unwrap()).unwrap();
        let loaded: Config = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();

        assert_eq!(loaded.recent_dirs[0], PathBuf::from("/music/rock"));
        assert_eq!(loaded.recent_dirs[1], PathBuf::from("/music/jazz"));
    }
}
