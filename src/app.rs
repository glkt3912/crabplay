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
}

impl AppState {
    pub fn new(tracks: Vec<TrackInfo>) -> Self {
        Self {
            tracks,
            selected: 0,
            player_state: PlayerState::Stopped,
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

    pub fn set_playing(&mut self) {
        self.player_state = PlayerState::Playing;
    }

    pub fn set_paused(&mut self) {
        self.player_state = PlayerState::Paused;
    }

    pub fn set_stopped(&mut self) {
        self.player_state = PlayerState::Stopped;
    }
}
