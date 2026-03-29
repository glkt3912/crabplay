use crate::models::TrackInfo;

pub enum PlayerState {
    Stopped,
    Playing,
    Paused,
}

pub struct AppState {
    pub tracks: Vec<TrackInfo>,
    pub selected: usize,
    pub player_state: PlayerState,
    /// 直近の再生エラーメッセージ。次の操作で自動クリアされる。
    pub last_error: Option<String>,
}

impl AppState {
    pub fn new(tracks: Vec<TrackInfo>) -> Self {
        Self {
            tracks,
            selected: 0,
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

    pub fn current(&self) -> Option<&TrackInfo> {
        self.tracks.get(self.selected)
    }
}
