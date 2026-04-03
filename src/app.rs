use std::collections::HashMap;
use std::path::PathBuf;

use crate::models::TrackInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepeatMode {
    Off,
    All,
    One,
}

impl RepeatMode {
    pub fn cycle(self) -> Self {
        match self {
            RepeatMode::Off => RepeatMode::All,
            RepeatMode::All => RepeatMode::One,
            RepeatMode::One => RepeatMode::Off,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            RepeatMode::Off => "Off",
            RepeatMode::All => "All",
            RepeatMode::One => "One",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerState {
    Stopped,
    Playing,
    Paused,
}

// PlayerState は Copy なので参照ではなく値で返す。呼び出し側で * デリファレンス不要。

pub struct AppState {
    pub tracks: Vec<TrackInfo>,
    pub selected: usize,
    /// 起動時のスキャンディレクトリ。ソース選択でディレクトリに戻る際に使用。
    pub source_dir: PathBuf,
    playing_index: Option<usize>,
    player_state: PlayerState,
    /// 直近の再生エラーメッセージ。
    pub last_error: Option<String>,
    /// エラー表示開始時刻（5秒後に自動クリア）。
    error_since: Option<std::time::Instant>,
    /// 操作成功などの情報メッセージ。
    pub info_msg: Option<String>,
    /// info_msg の表示開始時刻（3秒後に自動クリア）。
    info_since: Option<std::time::Instant>,
    /// プレイリスト（保存対象の曲リスト、tracks のインデックス列）。再生で消費されない。
    playlist: Vec<usize>,
    /// リピートモード。
    pub repeat: RepeatMode,
    /// load_and_play 直後の is_empty() 誤検知を防ぐための再生開始時刻。
    playback_started_at: Option<std::time::Instant>,
    /// 音量（0.0〜2.0、デフォルト 1.0）。
    pub volume: f32,
}

impl AppState {
    pub fn new(tracks: Vec<TrackInfo>, source_dir: PathBuf) -> Self {
        Self {
            tracks,
            selected: 0,
            source_dir,
            playing_index: None,
            player_state: PlayerState::Stopped,
            last_error: None,
            error_since: None,
            info_msg: None,
            info_since: None,
            playlist: Vec::new(),
            repeat: RepeatMode::Off,
            playback_started_at: None,
            volume: 1.0,
        }
    }

    pub fn next(&mut self) {
        if self.selected + 1 < self.tracks.len() {
            self.selected += 1;
        }
    }

    pub fn prev(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
        }
    }

    /// 現在再生中のトラック（カーソル位置ではなく実際に再生しているもの）。
    pub fn current(&self) -> Option<&TrackInfo> {
        self.playing_index.and_then(|i| self.tracks.get(i))
    }

    pub fn playing_index(&self) -> Option<usize> {
        self.playing_index
    }

    pub fn player_state(&self) -> PlayerState {
        self.player_state
    }

    /// 新規再生開始。selected を playing_index に設定し、is_empty() 誤検知防止のため開始時刻を記録する。
    pub fn set_playing(&mut self) {
        self.playing_index = Some(self.selected);
        self.player_state = PlayerState::Playing;
        self.playback_started_at = Some(std::time::Instant::now());
    }

    /// ポーズ解除。playing_index は変えずに状態だけ Playing に戻す。
    /// ポーズ解除は新規ロードではないため playback_started_at は更新しない。
    /// （更新すると残り時間の短い曲の終了検知が最大 500ms 遅延する）
    pub fn set_resumed(&mut self) {
        self.player_state = PlayerState::Playing;
    }

    pub fn set_paused(&mut self) {
        self.player_state = PlayerState::Paused;
        self.playback_started_at = None;
    }

    pub fn set_stopped(&mut self) {
        self.playing_index = None;
        self.player_state = PlayerState::Stopped;
        self.playback_started_at = None;
    }

    /// ソース切り替え時にトラック一覧と全再生状態をリセットする。
    /// `player.stop()` の呼び出しは呼び出し側の責務。
    /// このメソッド後に `set_info()` を呼ぶと通知メッセージを表示できる。
    pub fn replace_tracks(&mut self, tracks: Vec<TrackInfo>) {
        self.tracks = tracks;
        self.selected = 0;
        self.playing_index = None;
        self.player_state = PlayerState::Stopped;
        self.playback_started_at = None;
        self.playlist.clear();
        self.last_error = None;
        self.error_since = None;
        self.info_msg = None;
        self.info_since = None;
    }

    /// 再生開始（またはポーズ解除）から 500ms 以上経過したか。
    /// load_and_play 直後の一瞬 is_empty() が true になる誤検知を防ぐ。
    pub fn is_playback_settled(&self) -> bool {
        self.playback_started_at
            .map(|t| t.elapsed() >= std::time::Duration::from_millis(500))
            .unwrap_or(true)
    }

    /// 選択中のトラックをプレイリストに追加する。
    /// 追加された場合は true、既に存在する場合は false を返す。
    pub fn playlist_add_selected(&mut self) -> bool {
        if self.playlist.contains(&self.selected) {
            return false;
        }
        self.playlist.push(self.selected);
        true
    }

    /// プレイリストをクリアする。
    pub fn clear_playlist(&mut self) {
        self.playlist.clear();
    }

    pub fn playlist_len(&self) -> usize {
        self.playlist.len()
    }

    pub fn playlist_is_empty(&self) -> bool {
        self.playlist.is_empty()
    }

    /// プレイリスト内トラックのパス一覧を返す。
    pub fn playlist_paths(&self) -> Vec<std::path::PathBuf> {
        self.playlist
            .iter()
            .filter_map(|&i| self.tracks.get(i))
            .map(|t| t.path.clone())
            .collect()
    }

    /// トラックインデックス → プレイリスト内位置リスト（1始まり）の HashMap を返す。
    /// draw() でフレームごとに O(N×P) の走査を避けるため、一度だけ走査して構築する。
    pub fn playlist_badge_map(&self) -> HashMap<usize, Vec<usize>> {
        let mut map: HashMap<usize, Vec<usize>> = HashMap::new();
        for (pos, &idx) in self.playlist.iter().enumerate() {
            map.entry(idx).or_default().push(pos + 1);
        }
        map
    }

    /// リピートモードをサイクルする。
    pub fn cycle_repeat(&mut self) {
        self.repeat = self.repeat.cycle();
    }

    /// 音量を 5% 上げる（上限 200%）。
    pub fn volume_up(&mut self) {
        self.volume = (self.volume + 0.05).min(2.0);
    }

    /// 音量を 5% 下げる（下限 0%）。
    pub fn volume_down(&mut self) {
        self.volume = (self.volume - 0.05).max(0.0);
    }

    /// エラーメッセージをセットし、5秒タイムアウト用の時刻を記録する。
    pub fn set_error(&mut self, msg: String) {
        self.last_error = Some(msg);
        self.error_since = Some(std::time::Instant::now());
    }

    /// 情報メッセージをセットし、3秒タイムアウト用の時刻を記録する。
    pub fn set_info(&mut self, msg: String) {
        self.info_msg = Some(msg);
        self.info_since = Some(std::time::Instant::now());
    }

    /// info_msg（3秒）と last_error（5秒）の自動クリアを行う。イベントループの先頭で毎フレーム呼ぶ。
    pub fn tick_timeouts(&mut self) {
        if self
            .error_since
            .map(|t| t.elapsed() >= std::time::Duration::from_secs(5))
            .unwrap_or(false)
        {
            self.last_error = None;
            self.error_since = None;
        }
        if self
            .info_since
            .map(|t| t.elapsed() >= std::time::Duration::from_secs(3))
            .unwrap_or(false)
        {
            self.info_msg = None;
            self.info_since = None;
        }
    }

    /// エラー・情報メッセージを両方クリアする。
    pub fn clear_messages(&mut self) {
        self.last_error = None;
        self.error_since = None;
        self.info_msg = None;
        self.info_since = None;
    }

    /// 現在のトラック終了後に次へ進む。RepeatMode のみで制御。
    ///
    /// 1. `RepeatMode::One` — playing_index のトラックをリピート
    /// 2. `RepeatMode::All` — (playing_index + 1) % len でループ
    /// 3. `RepeatMode::Off` — 線形に次へ（末尾なら false を返す）
    ///
    /// 次のトラックが存在する場合は `selected` を更新して `true` を返す。
    pub fn advance(&mut self) -> bool {
        if self.repeat == RepeatMode::One {
            if let Some(idx) = self.playing_index {
                self.selected = idx;
                return true;
            }
            return false;
        }
        if self.repeat == RepeatMode::All {
            if self.tracks.is_empty() {
                return false;
            }
            let base = self.playing_index.unwrap_or(self.selected);
            self.selected = (base + 1) % self.tracks.len();
            return true;
        }
        // RepeatMode::Off
        if self.selected + 1 < self.tracks.len() {
            self.selected += 1;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::TrackInfo;
    use std::path::PathBuf;

    fn make_state(n: usize) -> AppState {
        let tracks = (0..n)
            .map(|i| TrackInfo {
                path: PathBuf::from(format!("/track{i}.mp3")),
                title: format!("Track {i}"),
                artist: "Artist".to_string(),
                album: "Album".to_string(),
                duration_secs: 180,
            })
            .collect();
        AppState::new(tracks, PathBuf::from("."))
    }

    #[test]
    fn replace_tracks_resets_all_state() {
        let mut state = make_state(3);
        state.selected = 2;
        state.set_playing();
        state.playlist_add_selected();
        state.set_error("err".to_string());
        let new_tracks = vec![TrackInfo {
            path: PathBuf::from("/new.mp3"),
            title: "New".to_string(),
            artist: "A".to_string(),
            album: "B".to_string(),
            duration_secs: 60,
        }];
        state.replace_tracks(new_tracks);
        assert_eq!(state.tracks.len(), 1);
        assert_eq!(state.selected, 0);
        assert_eq!(state.playing_index(), None);
        assert!(state.playlist_is_empty());
        assert!(state.last_error.is_none());
        assert!(state.info_msg.is_none());
    }

    #[test]
    fn replace_tracks_clears_playlist() {
        let mut state = make_state(3);
        state.selected = 1;
        state.playlist_add_selected();
        assert_eq!(state.playlist_len(), 1);
        state.replace_tracks(vec![]);
        assert!(state.playlist_is_empty());
    }

    #[test]
    fn playlist_add_dedup() {
        let mut state = make_state(3);
        state.selected = 0;
        assert!(state.playlist_add_selected());
        assert!(!state.playlist_add_selected()); // 重複はスキップ
        assert_eq!(state.playlist_len(), 1);
    }

    #[test]
    fn playlist_not_consumed_by_advance() {
        let mut state = make_state(3);
        state.selected = 0;
        state.playlist_add_selected();
        state.set_playing();
        state.advance();
        assert_eq!(state.playlist_len(), 1);
    }

    #[test]
    fn is_playback_settled_when_never_started() {
        let state = make_state(1);
        assert!(state.is_playback_settled());
    }

    #[test]
    fn advance_repeat_one_returns_false_when_not_playing() {
        let mut state = make_state(3);
        state.repeat = RepeatMode::One;
        assert!(!state.advance());
    }

    #[test]
    fn advance_repeat_all_loops_around() {
        let mut state = make_state(3);
        state.selected = 2;
        state.set_playing();
        state.repeat = RepeatMode::All;
        assert!(state.advance());
        assert_eq!(state.selected, 0);
    }

    #[test]
    fn advance_repeat_all_empty_tracks() {
        let mut state = AppState::new(vec![], PathBuf::from("."));
        state.repeat = RepeatMode::All;
        assert!(!state.advance());
    }

    #[test]
    fn advance_repeat_all_uses_playing_index_as_base() {
        let mut state = make_state(5);
        state.selected = 0;
        state.set_playing();
        state.selected = 3;
        state.repeat = RepeatMode::All;
        assert!(state.advance());
        assert_eq!(state.selected, 1);
    }

    #[test]
    fn advance_repeat_one_resets_cursor_to_playing_index() {
        let mut state = make_state(3);
        state.selected = 0;
        state.set_playing();
        state.selected = 2;
        state.repeat = RepeatMode::One;
        assert!(state.advance());
        assert_eq!(
            state.selected, 0,
            "RepeatMode::One は playing_index のトラックに戻す"
        );
    }
}
