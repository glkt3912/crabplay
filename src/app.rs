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
    /// 再生キュー（tracks のインデックス列）。
    pub queue: VecDeque<usize>,
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

    pub fn set_playing(&mut self) {
        self.playing_index = Some(self.selected);
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
    /// キューに項目があればそれを優先し、なければ RepeatMode に従う。
    /// 次のトラックが存在する場合は `selected` を更新して `true` を返す。
    pub fn advance(&mut self) -> bool {
        self.clear_messages();
        if let Some(idx) = self.queue.pop_front() {
            self.selected = idx;
            return true;
        }
        match self.repeat {
            RepeatMode::One => true,
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
