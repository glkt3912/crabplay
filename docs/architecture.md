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
  ├── pub mod app                ← AppState / PlayerState / RepeatMode
  ├── pub mod audio              ← Player (rodio)
  ├── pub mod cli                ← Args (clap)
  ├── pub mod error              ← AppError (thiserror) ← 他モジュールが参照
  ├── pub mod library            ← scanner / metadata
  ├── pub mod models             ← TrackInfo (serde)
  ├── pub mod output             ← OutputFormatter トレイト
  ├── pub mod playlist           ← Playlist (保存/読み込み)
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
              ├── clear_messages()  ← 全キーイベントの先頭で実行
              ├── match key.code
              │     ├── Enter/n/p → play_current()             再生ヘルパー
              │     │               └── load_and_play() 成功 → set_playing()
              │     │               └── load_and_play() 失敗 → last_error にセット + set_stopped()
              │     ├── Space  → Player::toggle_pause()
              │     │           ├── paused  → set_paused()
              │     │           └── resumed → set_resumed()  ※playing_index は変えない
              │     ├── ↑/↓   → AppState::next/prev()
              │     ├── a      → enqueue_selected() → info_msg にキュー件数表示
              │     ├── c      → clear_queue() → info_msg に "Queue cleared"
              │     ├── r      → cycle_repeat() → info_msg にモード表示
              │     ├── s      → save_playlist() → 保存先パスを info_msg / エラーを last_error
              │     └── q      → Player::stop() → break
              ├── 選択変更検知 → marquee_offset / tick リセット
              ├── 5フレームごと → marquee_offset += 1
              └── rodio::Sink::empty() == true（= 再生バッファ空 = トラック完了）
                    ├── clear_messages()
                    ├── advance() == true → play_current() + marquee リセット
                    └── advance() == false → set_stopped()
```

描画は `draw()` 関数で 3 ペインに分割:

- **トラックリスト** (上部 `Constraint::Min(3)`): `List` ウィジェット + `Scrollbar`。選択行ハイライト。長いタイトル・アーティスト名はマーキースクロール。各行末尾にキュー位置バッジ（後述）を表示。
- **Now Playing** (中段 `Constraint::Length(3)`): 再生状態・曲名・アーティスト・経過時間 / 合計時間。`info_msg` があれば緑色、`last_error` があれば赤色で優先表示。
- **キーバインド** (下段 `Constraint::Length(3)`): 現在の `repeat` モードをリアルタイム表示する動的文字列。

### マーキースクロール実装

```
marquee_slice(s: &str, offset: usize, max_width: usize) -> String
  ├── chars: Vec<char>  // 文字単位で分割
  ├── idx = offset % (total + 2)  // 末尾に2文字分の空白を挟んでループ
  └── while width < max_width:
        unicode_width::UnicodeWidthStr::width() で全角文字の表示幅を測りながら
        width + ch_width > max_width なら break、そうでなければ追加
        ※ ループ条件を幅ベースにすることで全角文字の境界を正確に処理する
```

## AppState の設計

```rust
pub struct AppState {
    pub tracks: Vec<TrackInfo>,
    pub selected: usize,
    player_state: PlayerState,   // 非公開、遷移メソッド経由で変更
    pub last_error: Option<String>,
    pub info_msg: Option<String>,
    queue: VecDeque<usize>,      // 非公開、アクセサ経由で操作
    pub repeat: RepeatMode,
}
```

`player_state` / `queue` は直接書き換え不可。以下のメソッドで操作する:

| メソッド | 役割 |
|---------|------|
| `set_playing()` | 新規再生開始。`selected` を `playing_index` に設定 |
| `set_resumed()` | ポーズ解除。`playing_index` は変えず状態だけ `Playing` に戻す |
| `set_paused()` / `set_stopped()` | 状態遷移 |
| `player_state()` | 読み取り専用アクセス |
| `enqueue_selected()` / `clear_queue()` | キュー操作 |
| `queue_len()` / `queue_is_empty()` / `queue_paths()` | キュー参照（読み取り専用） |
| `queue_positions_for(track_index)` | 指定インデックスがキューの何番目にあるかを `Vec<usize>`（1始まり）で返す。重複登録時は複数の位置を含む。最大3件に制限 |
| `queue_badge_map()` | トラックインデックス → キュー内位置リストの `HashMap<usize, Vec<usize>>` を O(Q) で構築して返す。`draw()` がフレームごとに1回だけ呼び出し、O(N×Q) の繰り返し走査を回避する |

`set_playing()` と `set_resumed()` を分けることで、ポーズ中にカーソルを別トラックへ移動してもポーズ解除時に ▶ マーカーがずれない。

### キューと RepeatMode

```
advance() の優先順位:
  1. RepeatMode::One  → キューを消費せず selected をそのまま（同じトラックをリピート）
  2. queue に項目あり → pop_front() した index を selected にセット
  3. RepeatMode::All  → (selected + 1) % tracks.len()
  4. RepeatMode::Off  → selected + 1（末尾なら false を返して停止）
```

RepeatMode::One を最優先にすることで、1曲リピート中にキューへ追加した曲が割り込まない。
メッセージのクリアは `advance()` ではなく呼び出し側（TUI の auto-advance ブロック）の責務とする。

### キュー位置バッジ

トラックリストの各行末尾に `BADGE_WIDTH = 6` 文字固定のバッジを表示する（Color::Magenta）。

```
  Bohemian Rhapsody    Queen     5:54        ← キューなし（空白 6 文字）
▶ Hotel California     Eagles    6:30  [1]   ← キュー 1 番目
  Stairway to Heaven   Led Zep   8:02  [2]   ← キュー 2 番目
  Hotel California     Eagles    6:30  [1,3] ← 1 番目と 3 番目に重複登録
  Comfortably Numb     Pink F    6:21  [2+2] ← 2 番目 + 残り 2 件
```

`format_queue_badge(positions: &[usize]) -> String` の変換規則:

| 状態 | 表示例 |
|------|--------|
| キューなし | `"      "` (空白 6 文字) |
| 1 箇所 | `"[1]   "` |
| 2 箇所かつ両方 1 桁 | `"[1,3] "` (`[x,y]` は 5 文字。両方 1 桁のときのみ `BADGE_WIDTH` に収まる) |
| それ以外（3 箇所以上 or 2 桁以上） | `"[1+2] "` (先頭位置 + 残り件数) |

文字列生成後に `.chars().take(BADGE_WIDTH)` で切り詰め、`format!("{:<6}", ...)` でパディングする。これにより `p1` や残り件数が 2 桁以上になっても `BADGE_WIDTH` を超えない。

幅を固定することで、ターミナル幅が狭い場合でも末尾から自然に切り詰められ、タイトル・アーティストなどの主要情報が保護される。

**パフォーマンス設計:**  
`draw()` はフレームごとに `queue_badge_map: &HashMap<usize, Vec<usize>>` を受け取る。呼び出し元 `event_loop` で `state.queue_badge_map()` を1回だけ呼び出してキャッシュし、トラックリストループ内は `map.get(&i)` の O(1) 参照のみ行う（旧来の O(N×Q) 走査を廃止）。

### Playlist モジュール

```rust
// src/playlist.rs
pub struct Playlist {
    pub name: String,
    pub paths: Vec<PathBuf>,
}
```

- `save(&dir)` — ファイル名を ASCII 英数字・`-`・`_` のみにサニタイズして `dir/<name>.json` に保存。サニタイズ後が空文字になる場合はエラーを返す
- `load(&path)` — JSON ファイルから復元
- `default_dir()` — `XDG_CONFIG_HOME` → `HOME/.config` → `.` の優先順で解決し、`crabplay/playlists/` を付加して返す

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
