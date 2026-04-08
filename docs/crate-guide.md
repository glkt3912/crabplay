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
- `Sink::get_pos() -> Duration` (v0.19+) で再生位置取得 → Now Playing 表示に使用
- `Sink::try_seek(Duration) -> Result<(), SeekError>` (v0.19+) で任意位置にシーク → `←/→` キー操作に使用
- `Sink` は内部で `Arc<Controls>` を使い `&self` で全操作が可能 → `Mutex` ラップ不要

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
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Scrollbar},
};

terminal.draw(|f| {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(3), Constraint::Length(3)])
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
- `Scrollbar` + `ScrollbarState` でスクロールインジケーター表示

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

- `enable_raw_mode()` は必ず `disable_raw_mode()` とペアで呼ぶ（エラー時も）→ Drop guard で保証
- `EnterAlternateScreen` で TUI 専用バッファに切り替え → 終了後に元の画面が復元される
- `KeyEventKind::Press` でキー押下のみを処理（リリース・リピートを除外）
- `event::poll(Duration)` でタイムアウト付き待機 → CPU を使い過ぎない

---

## unicode-width (v0.1)

**役割**: 文字列の表示幅（表示列数）を Unicode 規格に基づいて計算する。特にCJK（中日韓）全角文字の扱いに必要。

**使用箇所**: `src/ui/tui.rs` のマーキースクロール処理

```rust
use unicode_width::UnicodeWidthStr;

// ASCII: 1文字 = 幅1
// CJK全角: 1文字 = 幅2
let width = UnicodeWidthStr::width("King Gnu");  // 8
let width = UnicodeWidthStr::width("米津玄師");   // 8 (4文字 × 幅2)
```

**覚えておくべきポイント**:

- `str::len()` はバイト数、`str::chars().count()` は文字数を返すが、どちらも**表示幅ではない**
- 表示幅に基づいてスライスする場合は文字単位でループし、幅を累積する必要がある
- ratatui が内部で依存しているため、バージョンは 0.1.x に統一する（0.2.x と混在するとトレイト実装の競合が発生）

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

**覚えておくべきポイント**:

- `short` で `-d`、`long` で `--dir` を自動生成（フィールド名から）
- `default_value` は文字列、`default_value_t` は型付きの値
- `#[command(version)]` で `Cargo.toml` の `version` を `--version` に自動反映
- `derive` feature が必要 (`clap = { version = "4", features = ["derive"] }`)

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

**使い分けの指針**:

- ライブラリ・モジュール境界では `AppError` を使い、呼び出し元がエラーを識別・ハンドリングできるようにする
- `main.rs` など最終的にエラーを表示するだけの層では `anyhow::Result` で統一し、`.context()` で情報を追記する

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

---

## toml (v0.8)

**役割**: アプリ設定（`~/.config/crabplay/config.toml`）の読み書き。

**使用箇所**: `src/config.rs`

```rust
// 読み込み
let config: Config = toml::from_str(&std::fs::read_to_string(path)?)?;

// 書き出し
std::fs::write(path, toml::to_string(&config)?)?;
```

**覚えておくべきポイント**:

- `serde` の `Deserialize` / `Serialize` derive と組み合わせて使う
- `toml::from_str` はパース失敗時にエラーを返す → `Config::load()` では `.ok().unwrap_or_default()` でフォールバック
- `toml::to_string` は構造体のフィールドのみをシリアライズする（未知のフィールドは保持されない）
