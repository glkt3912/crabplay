use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 1曲分のメタデータ。`library::metadata::read_metadata` で生成され、アプリ全体で読み取り専用として扱う。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackInfo {
    /// 音声ファイルの絶対パス。
    pub path: PathBuf,
    /// トラックタイトル。タグが存在しない場合はファイル名（拡張子なし）を使用。
    pub title: String,
    /// アーティスト名。タグが存在しない場合は空文字。
    pub artist: String,
    /// アルバム名。タグが存在しない場合は空文字。
    pub album: String,
    /// 再生時間（秒）。
    pub duration_secs: u64,
}
