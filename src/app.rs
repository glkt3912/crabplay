use std::collections::{HashMap, VecDeque};

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

pub struct AppState {
    pub tracks: Vec<TrackInfo>,
    pub selected: usize,
    playing_index: Option<usize>,
    player_state: PlayerState,
    /// 直近の再生エラーメッセージ。
    pub last_error: Option<String>,
    /// エラー表示開始時刻（5秒後に自動クリア）。
    error_since: Option<std::time::Instant>,
    /// 操作成功などの情報メッセージ。
    pub info_msg: Option<String>,
    /// 再生キュー（tracks のインデックス列）。外部からは queue_len() / enqueue_selected() / clear_queue() を使う。
    queue: VecDeque<usize>,
    /// リピートモード。
    pub repeat: RepeatMode,
    /// load_and_play 直後の is_empty() 誤検知を防ぐための再生開始時刻。
    playback_started_at: Option<std::time::Instant>,
}

impl AppState {
    pub fn new(tracks: Vec<TrackInfo>) -> Self {
        Self {
            tracks,
            selected: 0,
            playing_index: None,
            player_state: PlayerState::Stopped,
            last_error: None,
            error_since: None,
            info_msg: None,
            queue: VecDeque::new(),
            repeat: RepeatMode::Off,
            playback_started_at: None,
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

    pub fn player_state(&self) -> &PlayerState {
        &self.player_state
    }

    /// 新規再生開始。selected を playing_index に設定し、is_empty() 誤検知防止のため開始時刻を記録する。
    pub fn set_playing(&mut self) {
        self.playing_index = Some(self.selected);
        self.player_state = PlayerState::Playing;
        self.playback_started_at = Some(std::time::Instant::now());
    }

    /// ポーズ解除。playing_index は変えずに状態だけ Playing に戻す。
    pub fn set_resumed(&mut self) {
        self.player_state = PlayerState::Playing;
        self.playback_started_at = Some(std::time::Instant::now());
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

    /// 再生開始（またはポーズ解除）から 500ms 以上経過したか。
    /// load_and_play 直後の一瞬 is_empty() が true になる誤検知を防ぐ。
    pub fn is_playback_settled(&self) -> bool {
        self.playback_started_at
            .map(|t| t.elapsed() >= std::time::Duration::from_millis(500))
            .unwrap_or(true)
    }

    /// 選択中のトラックをキューの末尾に追加する。
    pub fn enqueue_selected(&mut self) {
        self.queue.push_back(self.selected);
    }

    /// 再生キューをクリアする。
    pub fn clear_queue(&mut self) {
        self.queue.clear();
    }

    /// 現在のキュー件数を返す。
    pub fn queue_len(&self) -> usize {
        self.queue.len()
    }

    /// キューが空かどうかを返す。
    pub fn queue_is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// キューに積まれているトラックのパス一覧を返す。
    /// キューが空の場合は空の Vec を返す（全トラックへのフォールバックは呼び出し側で行う）。
    pub fn queue_paths(&self) -> Vec<std::path::PathBuf> {
        self.queue
            .iter()
            .filter_map(|&i| self.tracks.get(i))
            .map(|t| t.path.clone())
            .collect()
    }

    /// 指定したトラックインデックスが queue の何番目（1始まり）に存在するか返す。
    /// 表示用途のため最大3件に制限（format_queue_badge が使うのは先頭2件 + 残り件数のみ）。
    pub fn queue_positions_for(&self, track_index: usize) -> Vec<usize> {
        self.queue
            .iter()
            .enumerate()
            .filter(|&(_, &idx)| idx == track_index)
            .map(|(pos, _)| pos + 1)
            .take(3)
            .collect()
    }

    /// トラックインデックス → キュー内位置リスト（1始まり）の HashMap を返す。
    /// draw() でフレームごとに O(N×Q) の走査を避けるため、キュー全体を一度だけ走査して構築する。
    pub fn queue_badge_map(&self) -> HashMap<usize, Vec<usize>> {
        let mut map: HashMap<usize, Vec<usize>> = HashMap::new();
        for (pos, &idx) in self.queue.iter().enumerate() {
            map.entry(idx).or_default().push(pos + 1);
        }
        map
    }

    /// リピートモードをサイクルする。
    pub fn cycle_repeat(&mut self) {
        self.repeat = self.repeat.cycle();
    }

    /// エラーメッセージをセットし、5秒タイムアウト用の時刻を記録する。
    pub fn set_error(&mut self, msg: String) {
        self.last_error = Some(msg);
        self.error_since = Some(std::time::Instant::now());
    }

    /// エラーが 5秒以上表示されていれば自動クリアする。イベントループの先頭で毎フレーム呼ぶ。
    pub fn tick_error_timeout(&mut self) {
        if self
            .error_since
            .map(|t| t.elapsed() >= std::time::Duration::from_secs(5))
            .unwrap_or(false)
        {
            self.last_error = None;
            self.error_since = None;
        }
    }

    /// エラー・情報メッセージを両方クリアする。
    pub fn clear_messages(&mut self) {
        self.last_error = None;
        self.error_since = None;
        self.info_msg = None;
    }

    /// 現在のトラック終了後に次へ進む。
    ///
    /// 優先順位:
    /// 1. `RepeatMode::One` — playing_index のトラックをリピート（キューは消費しない）。
    ///    カーソル（selected）が移動していても playing_index に戻す。
    /// 2. キューに項目あり — pop_front() した index を selected にセット
    /// 3. `RepeatMode::All` — (selected + 1) % len でループ
    /// 4. `RepeatMode::Off` — 線形に次へ（末尾なら false を返す）
    ///
    /// 次のトラックが存在する場合は `selected` を更新して `true` を返す。
    /// メッセージのクリアは呼び出し側の責務とする。
    pub fn advance(&mut self) -> bool {
        if self.repeat == RepeatMode::One {
            // ループ対象は「現在再生中のトラック」。カーソルがずれても元のトラックに戻す。
            if let Some(idx) = self.playing_index {
                self.selected = idx;
            }
            return true;
        }
        if let Some(idx) = self.queue.pop_front() {
            self.selected = idx;
            return true;
        }
        if self.repeat == RepeatMode::All {
            if self.tracks.is_empty() {
                return false;
            }
            self.selected = (self.selected + 1) % self.tracks.len();
            true
        } else {
            // RepeatMode::Off
            if self.selected + 1 < self.tracks.len() {
                self.selected += 1;
                true
            } else {
                false
            }
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
        AppState::new(tracks)
    }

    #[test]
    fn queue_positions_empty_queue() {
        let state = make_state(3);
        assert_eq!(state.queue_positions_for(0), Vec::<usize>::new());
    }

    #[test]
    fn queue_positions_single() {
        let mut state = make_state(3);
        state.selected = 0;
        state.enqueue_selected();
        assert_eq!(state.queue_positions_for(0), vec![1]);
        assert_eq!(state.queue_positions_for(1), Vec::<usize>::new());
    }

    #[test]
    fn queue_positions_duplicate() {
        let mut state = make_state(3);
        state.selected = 0;
        state.enqueue_selected();
        state.enqueue_selected();
        state.enqueue_selected();
        assert_eq!(state.queue_positions_for(0), vec![1, 2, 3]);
    }

    #[test]
    fn advance_repeat_one_ignores_queue_and_resets_cursor() {
        let mut state = make_state(3);
        state.selected = 0;
        state.set_playing(); // playing_index = Some(0)
        // カーソルを動かしキューに積む
        state.selected = 1;
        state.enqueue_selected(); // queue: [1]
        state.selected = 2; // カーソルをさらに移動
        state.repeat = RepeatMode::One;
        // advance() はキューを無視し、selected を playing_index に戻す
        assert!(state.advance());
        assert_eq!(
            state.selected, 0,
            "RepeatMode::One は playing_index のトラックに戻す"
        );
        assert_eq!(state.queue_len(), 1, "キューは消費されない");
    }

    #[test]
    fn queue_positions_mixed() {
        let mut state = make_state(3);
        state.selected = 0;
        state.enqueue_selected(); // queue: [0]
        state.selected = 1;
        state.enqueue_selected(); // queue: [0, 1]
        state.selected = 0;
        state.enqueue_selected(); // queue: [0, 1, 0]
        assert_eq!(state.queue_positions_for(0), vec![1, 3]);
        assert_eq!(state.queue_positions_for(1), vec![2]);
        assert_eq!(state.queue_positions_for(2), Vec::<usize>::new());
    }
}
