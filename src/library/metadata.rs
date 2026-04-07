use std::path::Path;

use lofty::prelude::*;
use lofty::probe::Probe;

use crate::error::AppError;
use crate::models::TrackInfo;

/// 音声ファイルからメタデータを読み取り、[`TrackInfo`] を返す。
///
/// タイトルタグが存在しない場合はファイル名（拡張子なし）をタイトルとして使用する。
/// ファイルのオープンやデコードに失敗した場合は [`AppError::Metadata`] を返す。
pub fn read_metadata(path: &Path) -> Result<TrackInfo, AppError> {
    let tagged = Probe::open(path)
        .and_then(|p| p.read())
        .map_err(|e| AppError::Metadata {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;

    let tag = tagged.primary_tag();
    let title = tag
        .and_then(|t| t.title().map(|s| s.to_string()))
        .unwrap_or_else(|| {
            path.file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        });
    let artist = tag
        .and_then(|t| t.artist().map(|s| s.to_string()))
        .unwrap_or_default();
    let album = tag
        .and_then(|t| t.album().map(|s| s.to_string()))
        .unwrap_or_default();
    let duration_secs = tagged.properties().duration().as_secs();

    Ok(TrackInfo {
        path: path.to_owned(),
        title,
        artist,
        album,
        duration_secs,
    })
}
