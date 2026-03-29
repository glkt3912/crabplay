use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::sync::{Arc, Mutex};

use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};

use crate::error::AppError;

pub struct Player {
    _stream: OutputStream,
    _handle: OutputStreamHandle,
    sink: Arc<Mutex<Sink>>,
}

impl Player {
    pub fn new() -> Result<Self, AppError> {
        let (stream, handle) =
            OutputStream::try_default().map_err(|e| AppError::Audio(e.to_string()))?;
        let sink = Sink::try_new(&handle).map_err(|e| AppError::Audio(e.to_string()))?;
        Ok(Self {
            _stream: stream,
            _handle: handle,
            sink: Arc::new(Mutex::new(sink)),
        })
    }

    pub fn load_and_play(&self, path: &Path) -> Result<(), AppError> {
        let file = BufReader::new(File::open(path)?);
        let source = Decoder::new(file).map_err(|e| AppError::Audio(e.to_string()))?;
        let sink = self.sink.lock().unwrap();
        sink.stop();
        sink.append(source);
        sink.play();
        Ok(())
    }

    pub fn toggle_pause(&self) {
        let sink = self.sink.lock().unwrap();
        if sink.is_paused() {
            sink.play();
        } else {
            sink.pause();
        }
    }

    pub fn is_paused(&self) -> bool {
        self.sink.lock().unwrap().is_paused()
    }

    pub fn is_empty(&self) -> bool {
        self.sink.lock().unwrap().empty()
    }

    pub fn stop(&self) {
        self.sink.lock().unwrap().stop();
    }
}
