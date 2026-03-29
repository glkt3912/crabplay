# Extension Cookbook

crabplay に機能を追加する際の具体的な手順集。

---

## 1. 音量調整を追加する

**変更ファイル**: `src/audio/player.rs`, `src/ui/tui.rs`

```rust
// src/audio/player.rs
pub fn set_volume(&self, volume: f32) {
    self.sink.lock().unwrap().set_volume(volume.clamp(0.0, 1.0));
}

pub fn volume(&self) -> f32 {
    self.sink.lock().unwrap().volume()
}
```

TUI のキーバインドに追加:

```rust
// src/ui/tui.rs
KeyCode::Char('+') => player.set_volume(player.volume() + 0.1),
KeyCode::Char('-') => player.set_volume(player.volume() - 0.1),
```

---

## 2. 進捗バーを表示する

**変更ファイル**: `src/audio/player.rs`, `src/ui/tui.rs`

```rust
// src/audio/player.rs
pub fn position_secs(&self) -> u64 {
    self.sink.lock().unwrap().get_pos().as_secs()
}
```

TUI の描画に `Gauge` ウィジェットを追加:

```rust
// src/ui/tui.rs
use ratatui::widgets::Gauge;

let progress = if track.duration_secs > 0 {
    player.position_secs() as f64 / track.duration_secs as f64
} else {
    0.0
};

let gauge = Gauge::default()
    .block(Block::default().borders(Borders::ALL))
    .gauge_style(Style::default().fg(Color::Green))
    .ratio(progress.min(1.0));
```

---

## 3. シャッフル再生を追加する

**変更ファイル**: `src/app.rs`, `Cargo.toml`

```toml
# Cargo.toml
rand = "0.9"
```

```rust
// src/app.rs
use rand::seq::SliceRandom;

pub struct AppState {
    pub tracks: Vec<TrackInfo>,
    pub order: Vec<usize>,    // 再生順序
    pub queue_pos: usize,
    pub shuffle: bool,
    pub player_state: PlayerState,
}

impl AppState {
    pub fn toggle_shuffle(&mut self) {
        self.shuffle = !self.shuffle;
        if self.shuffle {
            self.order.shuffle(&mut rand::rng());
        } else {
            self.order = (0..self.tracks.len()).collect();
        }
    }

    pub fn current(&self) -> Option<&TrackInfo> {
        self.order.get(self.queue_pos).and_then(|&i| self.tracks.get(i))
    }
}
```

---

## 4. 出力形式を追加する（例: CSV）

**変更ファイル**: `src/output.rs`, `src/cli.rs`

```rust
// src/output.rs
pub struct CsvFormatter;

impl OutputFormatter for CsvFormatter {
    fn format_track(&self, track: &TrackInfo) -> Result<String, AppError> {
        Ok(format!(
            "{},{},{},{}",
            track.path.display(), track.title, track.artist, track.duration_secs
        ))
    }

    fn format_name(&self) -> &'static str { "csv" }
}

// make_formatter の match に追加
"csv" => Box::new(CsvFormatter),
```

`cli.rs` の `validate()` も更新:

```rust
match self.format.as_str() {
    "text" | "json" | "csv" => {}
    // ...
}
```

---

## 5. 対応フォーマットを追加する

**変更ファイル**: `src/library/scanner.rs`, `Cargo.toml`

```rust
// src/library/scanner.rs
let supported = ["mp3", "flac", "aac", "ogg", "wav"];
```

```toml
# Cargo.toml
rodio = { version = "0.19", default-features = false, features = [
    "symphonia-mp3",
    "symphonia-flac",
    "symphonia-aac",
    "symphonia-vorbis",
    "symphonia-wav",
] }
```

---

## 6. 設定ファイルのサポート

**新規ファイル**: `src/config.rs`

```toml
# Cargo.toml
toml = "0.8"
dirs = "5"
```

```rust
// src/config.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Default)]
pub struct Config {
    pub music_dir: Option<String>,
    pub default_format: Option<String>,
}

pub fn load_config() -> Config {
    let path = dirs::config_dir()
        .map(|d| d.join("crabplay/config.toml"));

    path.and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default()
}
```

`main.rs` で Config を先に読み込み、CLI フラグで上書きするパターンで使う。

---

## 7. GUI 化する（iced）

**変更ファイル**: `src/ui/` 全体, `Cargo.toml`

`src/ui/tui.rs` を `src/ui/gui.rs` に差し替え、`iced` を使う。

```toml
iced = "0.13"
```

`ui/mod.rs` のエクスポートを変更するだけで `main.rs` の変更は最小限になる:

```rust
// src/ui/mod.rs
// pub mod tui;   // コメントアウト
pub mod gui;     // 新規

pub use gui::run; // インターフェースは同じ
```
