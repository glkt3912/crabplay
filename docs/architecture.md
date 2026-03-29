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
    sink: Arc<Mutex<Sink>>,     // スレッド安全な再生制御
}
```

`rodio` の `OutputStream` はライフタイムと紐づいており、drop されると音声が停止する。
`_stream` フィールドとして Player に保持することで、Player が生きている間は再生を維持する。

`Sink` を `Arc<Mutex<>>` でラップすることで、TUI スレッドからの排他的な再生制御を実現する。

## TUI アーキテクチャ

```
ui::tui::run()
  ├── enable_raw_mode()           キーボードの raw モード有効化
  ├── Terminal::new()             ratatui ターミナル初期化
  └── event_loop()
        ├── terminal.draw(|f| draw(f, state, list_state))   描画
        └── event::poll(200ms)    キーイベント待機
              └── match key.code
                    ├── Enter  → Player::load_and_play()
                    ├── Space  → Player::toggle_pause()
                    ├── n/p    → AppState::next/prev() + load_and_play()
                    ├── ↑/↓   → AppState::next/prev()
                    └── q      → Player::stop() → break
```

描画は `draw()` 関数で以下の 2 ペインに分割:
- **トラックリスト** (上部): `List` ウィジェット、選択行をハイライト
- **ステータスバー** (下部 3 行): 再生状態・曲名・キーバインド

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
