# Crate Guide

各依存クレートの役割、選定理由、典型的な使い方。

---

## rodio (v0.19)

**役割**: 音声ファイルの再生。`cpal` を経由して macOS の CoreAudio を使用する。

**使用箇所**: `src/audio/player.rs`

```rust
use rodio::{Decoder, OutputStream, Sink};

let (stream, handle) = OutputStream::try_default()?;
let sink = Sink::try_new(&handle)?;

let file = BufReader::new(File::open(path)?);
let source = Decoder::new(file)?;   // symphonia がデコード
sink.append(source);
sink.play();
```

**覚えておくべきポイント**:

- `OutputStream` は drop されると音が止まる → Player フィールドに保持必須
- `Sink::stop()` で現在の曲を停止 (`append` 前に呼ぶと前曲をクリアできる)
- `Sink::pause()` / `Sink::play()` で一時停止・再開
- `Sink::is_paused()` / `Sink::empty()` で状態確認
- `Sink::set_volume(f32)` で音量調整 (0.0〜1.0)
- `Sink::get_pos()` (rodio 0.19+) で再生位置取得 → 進捗バーに利用可能

**feature フラグ**:

```toml
rodio = { version = "0.19", default-features = false, features = ["symphonia-mp3", "symphonia-flac"] }
```

`default-features = false` にして `symphonia-*` のみ有効化することで、外部 C ライブラリを使わない Pure Rust デコードを実現。

---

## lofty (v0.21)

**役割**: 音声ファイルのメタデータ（ID3 タグ、Vorbis コメント等）の読み書き。

**使用箇所**: `src/library/metadata.rs`

```rust
use lofty::prelude::*;
use lofty::probe::Probe;

let tagged = Probe::open(path)?.read()?;
let tag = tagged.primary_tag();

let title  = tag.and_then(|t| t.title().map(|s| s.to_string()));
let artist = tag.and_then(|t| t.artist().map(|s| s.to_string()));
let album  = tag.and_then(|t| t.album().map(|s| s.to_string()));
let duration = tagged.properties().duration().as_secs();
```

**覚えておくべきポイント**:

- `Probe::open().read()` でファイル形式を自動判定して読み込む
- `primary_tag()` は最も優先度の高いタグを返す (MP3 なら ID3v2、FLAC なら VorbisComment)
- `tags()` で全タグを取得できる
- 書き込みは `tagged_file.save_to_path()` で可能
- タグが存在しない場合は `None` を返す → `unwrap_or_default()` で安全に扱う

---

## walkdir (v2)

**役割**: ディレクトリを再帰的に走査して音声ファイルを収集。

**使用箇所**: `src/library/scanner.rs`

```rust
use walkdir::WalkDir;

WalkDir::new(dir)
    .follow_links(true)
    .into_iter()
    .filter_map(|e| e.ok())          // アクセス不可ファイルをスキップ
    .filter(|e| is_audio_file(e.path()))
    .map(|e| e.path().to_owned())
    .collect()
```

**覚えておくべきポイント**:

- `.follow_links(true)` でシンボリックリンクも追跡
- `.max_depth(N)` で再帰深さを制限できる
- `filter_map(|e| e.ok())` で権限エラーのファイルを無視する
- `e.file_type().is_file()` でファイルのみに絞り込み可能

---

## ratatui (v0.28)

**役割**: TUI（ターミナルユーザーインターフェース）のレイアウトとウィジェット描画。

**使用箇所**: `src/ui/tui.rs`

```rust
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
};

terminal.draw(|f| {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(3)])
        .split(f.area());

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" crabplay "));
    f.render_stateful_widget(list, chunks[0], &mut list_state);
})?;
```

**覚えておくべきポイント**:

- `Layout` でペインを分割、`Constraint::Min` / `Length` / `Percentage` で比率指定
- `ListState` で選択状態を管理 → `render_stateful_widget` で渡す
- `Line` / `Span` でインライン色付きテキストを構築
- `Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)` でスタイル指定
- `f.area()` でターミナルサイズ取得

---

## crossterm (v0.28)

**役割**: ターミナルの raw モード制御、キーボードイベント取得。

**使用箇所**: `src/ui/tui.rs`

```rust
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{enable_raw_mode, disable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};

enable_raw_mode()?;
execute!(stdout, EnterAlternateScreen)?;
// ... TUI ループ ...
disable_raw_mode()?;
execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
```

```rust
if event::poll(std::time::Duration::from_millis(200))? {
    if let Event::Key(key) = event::read()? {
        if key.kind != KeyEventKind::Press { continue; }
        match key.code {
            KeyCode::Char('q') => break,
            KeyCode::Enter     => { /* 再生 */ }
            _                  => {}
        }
    }
}
```

**覚えておくべきポイント**:

- `enable_raw_mode()` は必ず `disable_raw_mode()` とペアで呼ぶ（エラー時も）
- `EnterAlternateScreen` で TUI 専用バッファに切り替え → 終了後に元の画面が復元される
- `KeyEventKind::Press` でキー押下のみを処理（リリース・リピートを除外）
- `event::poll(Duration)` でタイムアウト付き待機 → CPU を使い過ぎない

---

## clap (v4, derive)

**役割**: コマンドライン引数のパースとヘルプ生成。

**使用箇所**: `src/cli.rs`

```rust
#[derive(Parser, Debug)]
#[command(name = "crabplay", version, about)]
pub struct Args {
    #[arg(short, long, default_value = ".")]
    pub dir: PathBuf,

    #[arg(short, long, default_value = "text")]
    pub format: String,

    #[arg(short, long, default_value_t = false)]
    pub list: bool,
}
```

---

## anyhow / thiserror

**役割**: 二層エラーハンドリング。

- `thiserror`: `src/error.rs` の `AppError` 定義 → ライブラリ層
- `anyhow`: `src/main.rs` の `run()` → アプリ層でエラーチェインを構築

```rust
// ライブラリ層: 型安全なエラー
fn scan_directory(dir: &Path) -> Result<Vec<PathBuf>, AppError>

// アプリ層: .context() で文脈を付加
let paths = scan_directory(&args.dir).context("directory scan failed")?;
```

---

## serde / serde_json

**役割**: `TrackInfo` の JSON シリアライズ（`--list --format json` 出力）。

**使用箇所**: `src/models.rs`, `src/output.rs`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackInfo {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_secs: u64,
}

// JSON 出力
serde_json::to_string_pretty(track)?
```
