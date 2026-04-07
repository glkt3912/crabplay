use crate::error::AppError;
use crate::models::TrackInfo;

/// `--list` モードでのトラック出力フォーマッタ。
pub trait OutputFormatter {
    /// トラックを文字列にフォーマットする。
    fn format_track(&self, track: &TrackInfo) -> Result<String, AppError>;
    /// このフォーマッタの識別名を返す（`"text"` / `"json"` 等）。
    fn format_name(&self) -> &'static str;
}

/// `[Artist] Title (M:SS)` 形式でテキスト出力するフォーマッタ。
pub struct TextFormatter;

impl OutputFormatter for TextFormatter {
    fn format_track(&self, track: &TrackInfo) -> Result<String, AppError> {
        let mins = track.duration_secs / 60;
        let secs = track.duration_secs % 60;
        let artist = if track.artist.is_empty() {
            "Unknown".to_string()
        } else {
            track.artist.clone()
        };
        Ok(format!(
            "[{}] {} ({}:{:02})",
            artist, track.title, mins, secs
        ))
    }

    fn format_name(&self) -> &'static str {
        "text"
    }
}

/// JSON 形式（`serde_json::to_string_pretty`）でトラックを出力するフォーマッタ。
pub struct JsonFormatter;

impl OutputFormatter for JsonFormatter {
    fn format_track(&self, track: &TrackInfo) -> Result<String, AppError> {
        serde_json::to_string_pretty(track)
            .map_err(|e| AppError::Other(format!("JSON serialization failed: {e}")))
    }

    fn format_name(&self) -> &'static str {
        "json"
    }
}

/// フォーマット名から対応する [`OutputFormatter`] を生成する。
/// 未知のフォーマット名は `TextFormatter` にフォールバックする。
pub fn make_formatter(format: &str) -> Box<dyn OutputFormatter> {
    match format {
        "json" => Box::new(JsonFormatter),
        _ => Box::new(TextFormatter),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn sample_track() -> TrackInfo {
        TrackInfo {
            path: PathBuf::from("/music/test.mp3"),
            title: "Test Song".to_string(),
            artist: "Artist".to_string(),
            album: "Album".to_string(),
            duration_secs: 183,
        }
    }

    #[test]
    fn text_formatter_formats_track() {
        let result = TextFormatter.format_track(&sample_track()).unwrap();
        assert!(result.contains("Test Song"));
        assert!(result.contains("Artist"));
    }

    #[test]
    fn json_formatter_produces_valid_json() {
        let result = JsonFormatter.format_track(&sample_track()).unwrap();
        let _: serde_json::Value = serde_json::from_str(&result).unwrap();
    }
}
