use crate::models::TrackInfo;

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
    /// 直近の再生エラーメッセージ。次の操作で自動クリアされる。
    pub last_error: Option<String>,
}

impl AppState {
    pub fn new(tracks: Vec<TrackInfo>) -> Self {
        Self {
            tracks,
            selected: 0,
            playing_index: None,
            player_state: PlayerState::Stopped,
            last_error: None,
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

    /// 現在再生中のトラック（カーソル位置ではなく実際に再生しているもの）
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
}
