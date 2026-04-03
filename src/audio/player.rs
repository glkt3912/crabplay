use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};

use crate::error::AppError;

pub struct Player {
    _stream: OutputStream,
    _handle: OutputStreamHandle,
    sink: Sink,
}

impl Player {
    pub fn new() -> Result<Self, AppError> {
        let (stream, handle) =
            OutputStream::try_default().map_err(|e| AppError::Audio(e.to_string()))?;
        let sink = Sink::try_new(&handle).map_err(|e| AppError::Audio(e.to_string()))?;
        Ok(Self {
            _stream: stream,
            _handle: handle,
            sink,
        })
    }

    pub fn load_and_play(&self, path: &Path) -> Result<(), AppError> {
        let file = BufReader::new(File::open(path)?);
        let source = Decoder::new(file).map_err(|e| AppError::Audio(e.to_string()))?;
        self.sink.stop();
        self.sink.append(source);
        self.sink.play();
        Ok(())
    }

    /// ポーズ/再開を切り替え、切り替え後に「ポーズ中か」を返す。
    pub fn toggle_pause(&self) -> bool {
        if self.sink.is_paused() {
            self.sink.play();
            false
        } else {
            self.sink.pause();
            true
        }
    }

    pub fn is_empty(&self) -> bool {
        self.sink.empty()
    }

    pub fn stop(&self) {
        self.sink.stop();
    }

    pub fn get_pos(&self) -> std::time::Duration {
        self.sink.get_pos()
    }

    pub fn set_volume(&self, volume: f32) {
        self.sink.set_volume(volume);
    }
}
