use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink};

use crate::error::AppError;

/// `rodio` の `Sink` をラップした音声プレイヤー。
///
/// `_stream` / `_handle` は Drop 時に出力デバイスを解放するため保持する。
/// `Player` は `Clone` を実装しないため、アプリ全体で1つだけ生成して参照渡しする。
pub struct Player {
    _stream: OutputStream,
    _handle: OutputStreamHandle,
    sink: Sink,
}

impl Player {
    /// デフォルトの音声出力デバイスで `Player` を初期化する。
    ///
    /// デバイスが見つからない場合は [`AppError::Audio`] を返す。
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

    /// 指定パスの音声ファイルを読み込み、再生を開始する。
    ///
    /// 既に再生中のトラックは即座に停止され、新しいトラックに切り替わる。
    /// ファイルのオープンやデコードに失敗した場合は [`AppError`] を返す。
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

    /// 再生バッファが空（＝トラック再生完了またはロード前）かどうかを返す。
    pub fn is_empty(&self) -> bool {
        self.sink.empty()
    }

    /// 再生を即座に停止し、バッファをクリアする。
    pub fn stop(&self) {
        self.sink.stop();
    }

    /// 現在の再生位置を返す。
    pub fn get_pos(&self) -> std::time::Duration {
        self.sink.get_pos()
    }

    /// 指定位置にシークする。トラックの長さを超えた場合は末尾にクランプされる。
    ///
    /// 再生中・一時停止中どちらでも有効。停止中は何もしない。
    pub fn seek(&self, pos: std::time::Duration) -> Result<(), AppError> {
        self.sink
            .try_seek(pos)
            .map_err(|e| AppError::Audio(e.to_string()))
    }

    /// 音量を設定する（`0.0` = 無音、`1.0` = 標準、`2.0` = 最大）。
    pub fn set_volume(&self, volume: f32) {
        self.sink.set_volume(volume);
    }
}
