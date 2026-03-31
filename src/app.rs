use std::collections::VecDeque;

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
    /// 操作成功などの情報メッセージ。
    pub info_msg: Option<String>,
    /// 再生キュー（tracks のインデックス列）。外部からは queue_len() / enqueue_selected() / clear_queue() を使う。
    queue: VecDeque<usize>,
    /// リピートモード。
    pub repeat: RepeatMode,
}

impl AppState {
    pub fn new(tracks: Vec<TrackInfo>) -> Self {
        Self {
            tracks,
            selected: 0,
            playing_index: None,
            player_state: PlayerState::Stopped,
            last_error: None,
            info_msg: None,
            queue: VecDeque::new(),
            repeat: RepeatMode::Off,
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

    /// 新規再生開始。selected を playing_index に設定する。
    pub fn set_playing(&mut self) {
        self.playing_index = Some(self.selected);
        self.player_state = PlayerState::Playing;
    }

    /// ポーズ解除。playing_index は変えずに状態だけ Playing に戻す。
    pub fn set_resumed(&mut self) {
        self.player_state = PlayerState::Playing;
    }

    pub fn set_paused(&mut self) {
        self.player_state = PlayerState::Paused;
    }

    pub fn set_stopped(&mut self) {
        self.playing_index = None;
        self.player_state = PlayerState::Stopped;
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

    /// リピートモードをサイクルする。
    pub fn cycle_repeat(&mut self) {
        self.repeat = self.repeat.cycle();
    }

    /// エラー・情報メッセージを両方クリアする。
    pub fn clear_messages(&mut self) {
        self.last_error = None;
        self.info_msg = None;
    }

    /// 現在のトラック終了後に次へ進む。
    ///
    /// 優先順位:
    /// 1. `RepeatMode::One` — selected を変えずにそのままリピート（キューは消費しない）
    /// 2. キューに項目あり — pop_front() した index を selected にセット
    /// 3. `RepeatMode::All` — (selected + 1) % len でループ
    /// 4. `RepeatMode::Off` — 線形に次へ（末尾なら false を返す）
    ///
    /// 次のトラックが存在する場合は `selected` を更新して `true` を返す。
    /// メッセージのクリアは呼び出し側の責務とする。
    pub fn advance(&mut self) -> bool {
        if self.repeat == RepeatMode::One {
            return true;
        }
        if let Some(idx) = self.queue.pop_front() {
            self.selected = idx;
            return true;
        }
        match self.repeat {
            RepeatMode::One => unreachable!(),
            RepeatMode::All => {
                if self.tracks.is_empty() {
                    return false;
                }
                self.selected = (self.selected + 1) % self.tracks.len();
                true
            }
            RepeatMode::Off => {
                if self.selected + 1 < self.tracks.len() {
                    self.selected += 1;
                    true
                } else {
                    false
                }
            }
        }
    }
}
