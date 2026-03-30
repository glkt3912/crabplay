# Architecture

## モジュール依存関係

```
main.rs
  ├── cli::Args                  (引数パース + validate)
  ├── library::scanner           (ディレクトリスキャン)
  ├── library::metadata          (メタデータ読み取り)
  ├── audio::player::Player      (再生エンジン)
  ├── app::AppState              (アプリケーション状態)
  └── ui::tui                    (TUI レンダリング + イベントループ)

lib.rs (モジュール宣言)
  ├── pub mod app                ← AppState / PlayerState
  ├── pub mod audio              ← Player (rodio)
  ├── pub mod cli                ← Args (clap)
  ├── pub mod error              ← AppError (thiserror) ← 他モジュールが参照
  ├── pub mod library            ← scanner / metadata
  ├── pub mod models             ← TrackInfo (serde)
  ├── pub mod output             ← OutputFormatter トレイト
  └── pub mod ui                 ← tui (ratatui)
```

## エラーハンドリング

ライブラリ層 (`thiserror`) とアプリケーション層 (`anyhow`) を分離している。

```
┌────────────────────────────────────────────────┐
│  main.rs  (アプリケーション層)                   │
│                                                │
│  anyhow::Result<()>                            │
│    ├── .context("...") でエラーに文脈を付与      │
│    └── main() で catch → stderr 表示            │
├────────────────────────────────────────────────┤
│  error.rs  (ライブラリ層)                        │
│                                                │
│  AppError (thiserror)                          │
│    ├── Audio(String)          再生・初期化失敗   │
│    ├── Metadata { path, msg } タグ読み取り失敗   │
│    ├── Scan(String)           スキャン失敗       │
│    ├── Io(#[from] io::Error)  ファイル I/O      │
│    └── Other(String)          その他            │
└────────────────────────────────────────────────┘
```

## データフロー

```
CLI 引数
  └─ Args::parse() → Args::validate()
                          │
                          ▼
            scan_directory(&dir) → Vec<PathBuf>
                          │
                          ▼
            read_metadata(path) × N → Vec<TrackInfo>
                          │
              ┌───────────┴───────────┐
              ▼                       ▼
        --list フラグあり         TUI モード
        OutputFormatter           AppState::new(tracks)
        format_track()                │
        stdout 出力               ui::tui::run()
                                      │
                                  キー入力
                                      │
                              Player::load_and_play()
                                      │
                                  CoreAudio 出力
```

## 再生エンジンの構造

```rust
// audio/player.rs
pub struct Player {
    _stream: OutputStream,      // drop されると音が止まるため保持
    _handle: OutputStreamHandle,
    sink: Sink,                 // rodio::Sink は内部で Arc<Controls> を持つ
}
```

`rodio` の `OutputStream` はライフタイムと紐づいており、drop されると音声が停止する。
`_stream` フィールドとして Player に保持することで、Player が生きている間は再生を維持する。

`rodio::Sink` のメソッド（`stop`, `append`, `play`, `pause`, `get_pos` など）はすべて `&self` を受け取り、
内部で `Arc<Controls>` による同期を行う。そのため `Mutex` による追加ラップは不要。

## TUI アーキテクチャ

```
ui::tui::run()
  ├── enable_raw_mode()           キーボードの raw モード有効化
  ├── TerminalGuard（Drop guard） パニック時も必ずターミナルを復元
  │     └── drop() → disable_raw_mode() + LeaveAlternateScreen + cursor::Show
  ├── Terminal::new()             ratatui ターミナル初期化
  └── event_loop()
        ├── marquee_offset / marquee_tick  マーキースクロール状態（ローカル変数）
        ├── terminal.draw(|f| draw(f, state, player, list_state, marquee_offset))
        └── event::poll(200ms)    キーイベント待機
              ├── match key.code
              │     ├── Enter/n/p → play_current()          再生ヘルパー
              │     │               └── load_and_play() 成功 → set_playing()
              │     │               └── load_and_play() 失敗 → last_error にセット
              │     ├── Space  → Player::toggle_pause() + set_paused/set_playing()
              │     ├── ↑/↓   → AppState::next/prev()
              │     └── q      → Player::stop() → break
              ├── 選択変更検知 → marquee_offset / tick リセット
              ├── 5フレームごと → marquee_offset += 1
              └── PlayerState::Playing && player.is_empty()
                    ├── 次トラックあり → next() + play_current() + marquee リセット
                    └── 最後のトラック → set_stopped()
```

描画は `draw()` 関数で 3 ペインに分割:
- **トラックリスト** (上部 `Constraint::Min(3)`): `List` ウィジェット + `Scrollbar`。選択行ハイライト。長いタイトル・アーティスト名はマーキースクロール。
- **Now Playing** (中段 `Constraint::Length(3)`): 再生状態・曲名・アーティスト・経過時間 / 合計時間。エラー発生時は赤色で表示。
- **キーバインド** (下段 `Constraint::Length(3)`): 固定文字列。

### マーキースクロール実装

```
marquee_slice(s: &str, offset: usize, max_width: usize) -> String
  ├── chars: Vec<char>  // 文字単位で分割
  ├── idx = offset % (total + 2)  // 末尾に2文字分の空白を挟んでループ
  └── unicode_width::UnicodeWidthStr::width() で全角文字の表示幅を考慮しながら
      max_width に収まるまで文字を追加
```

## AppState の設計

```rust
pub struct AppState {
    pub tracks: Vec<TrackInfo>,
    pub selected: usize,
    player_state: PlayerState,          // 非公開、遷移メソッド経由で変更
    pub last_error: Option<String>,     // 直近の再生エラー（次の操作でクリア）
}
```

`player_state` は直接書き換え不可。以下のメソッドで遷移する:
- `set_playing()` / `set_paused()` / `set_stopped()`
- `player_state()` で読み取り専用アクセス

これにより状態遷移のロジックが TUI 層に漏れることを防ぐ。

## OutputFormatter トレイト

```
OutputFormatter (trait)
  ├── format_track(&self, track: &TrackInfo) -> Result<String, AppError>
  └── format_name(&self) -> &'static str
        │
        ├── TextFormatter   → "[Artist] Title (M:SS)"
        └── JsonFormatter   → serde_json::to_string_pretty
```

`make_formatter(format: &str) -> Box<dyn OutputFormatter>` でファクトリを提供。
新フォーマット追加時は `output.rs` に struct + impl を追加し、`make_formatter` の match に追加するだけ。
