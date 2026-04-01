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

`toggle_pause()` はポーズ/再開を切り替え、**切り替え後の状態（`true` = ポーズ中）** を返す。
呼び出し側は戻り値だけで `set_paused()` / `set_resumed()` を選択できるため、直後に `is_paused()` を呼ぶ二段階同期が不要。

## TUI アーキテクチャ

```
ui::tui::run()
  ├── enable_raw_mode()           キーボードの raw モード有効化
  ├── TerminalGuard（Drop guard） パニック時も必ずターミナルを復元
  │     └── drop() → disable_raw_mode() + LeaveAlternateScreen + cursor::Show
  ├── Terminal::new()             ratatui ターミナル初期化
  └── event_loop()
        ├── marquee_offset / marquee_tick  マーキースクロール状態（ローカル変数）
        ├── queue_badge_map / queue_dirty  バッジキャッシュ（キュー変更時のみ再計算）
        ├── 各フレーム先頭で:
        │     ├── tick_timeouts()  info_msg 3秒 / last_error 5秒 を過ぎたら自動クリア
        │     └── queue_dirty == true なら queue_badge_map を再計算してフラグをリセット
        ├── terminal.draw(|f| draw(f, ..., &queue_badge_map))
        └── event::poll(200ms)    キーイベント待機
              ├── clear_messages()  ← 全キーイベントの先頭で実行
              ├── match key.code
              │     ├── Enter/n/p → play_current()             再生ヘルパー
              │     │               ├── clear_messages() で last_error/error_since/info_msg を全クリア
              │     │               └── load_and_play() 成功 → set_playing()（playback_started_at を記録）
              │     │               └── load_and_play() 失敗 → set_error() + set_stopped()
              │     ├── Space  → PlayerState::Stopped のときは無視（空 Sink への play() による状態破壊を防止）
              │     │           Player::toggle_pause() → bool 戻り値で分岐
              │     │           ├── true  → set_paused()
              │     │           └── false → set_resumed()  ※playing_index/playback_started_at は変えない
              │     ├── ↑/↓   → AppState::next/prev()
              │     ├── a      → enqueue_selected() + queue_dirty = true → set_info() でキュー件数（3秒表示）
              │     ├── c      → clear_queue() + queue_dirty = true → set_info() で "Queue cleared"（3秒）
              │     ├── r      → cycle_repeat() → set_info() でモード表示（3秒）
              │     ├── s      → save_playlist() → set_info() で保存先パス（3秒）/ set_error() でエラー（5秒）
              │     └── q      → Player::stop() → break
              ├── 選択変更検知 → marquee_offset / tick リセット
              ├── 5フレームごと → marquee_offset += 1
              └── is_playback_settled() && rodio::Sink::empty()（再生バッファ空 = トラック完了）
                    ├── ※ is_playback_settled(): load_and_play 直後 500ms は is_empty() 誤検知を防ぐ
                    ├── clear_messages()
                    ├── advance() == true → queue_dirty = true + play_current() + marquee リセット
                    └── advance() == false → set_stopped()
```

描画は `draw()` 関数で 3 ペインに分割:

- **トラックリスト** (上部 `Constraint::Min(3)`): `List` ウィジェット + `Scrollbar`。選択行ハイライト。長いタイトル・アーティスト名はマーキースクロール。各行末尾にキュー位置バッジ（後述）を表示。
- **Now Playing** (中段 `Constraint::Length(3)`): 再生状態・曲名・アーティスト・経過時間 / 合計時間。`info_msg` があれば緑色、`last_error` があれば赤色で優先表示。
- **キーバインド** (下段 `Constraint::Length(3)`): 現在の `repeat` モードをリアルタイム表示する動的文字列。

### マーキースクロール実装

```
marquee_slice(s: &str, offset: usize, max_width: usize) -> String
  ├── col_table: Vec<(累積開始列, char, 表示幅)>  // UnicodeWidthChar::width() で各文字の表示幅を計算
  ├── total_disp = Σ 表示幅          // 文字列全体の表示幅（列数）
  ├── loop_disp = total_disp + 2     // ループ幅 = 表示幅 + 2列の空白ギャップ
  ├── start_col = offset % loop_disp // offset は表示列単位（1増加 = 1列スクロール）
  └── while out_width < max_width:
        col % loop_disp が total_disp 以上 → 空白（ギャップ領域）
        それ以外 → col_table を線形探索して pos 列の文字を取得
          ├── pos == c_start（文字の先頭列）→ 文字を出力、col += 表示幅
          └── pos > c_start（全角文字の中間列に offset が着地）→ 空白1列を出力して
                col を c_start + w（次の文字の先頭）へ進める
        ※ offset を表示列ベースにすることで CJK 全角文字（1char = 2列）でも
          ASCII と同じ速度でスクロールする（旧実装: chars.len() ベースで 2 倍速になっていた）
```

**CJK 対応パディング (`pad_display`):**  
`format!("{:<N}", s)` は char 単位でパディングするため、全角文字を含む文字列では実際の表示列数が `N` を超える。`pad_display(s, width)` は `UnicodeWidthChar::width()` で表示幅を計算し、過不足なく `width` 列に揃える。マーキーを使わない非選択行のタイトル・アーティスト列に適用。  
表示幅取得には `UnicodeWidthChar::width(ch)` を使用（`encode_utf8` バッファ不要）。

## AppState の設計

```rust
pub struct AppState {
    pub tracks: Vec<TrackInfo>,
    pub selected: usize,
    player_state: PlayerState,       // 非公開、遷移メソッド経由で変更
    pub last_error: Option<String>,
    error_since: Option<Instant>,    // last_error の表示開始時刻（5秒タイムアウト用）
    pub info_msg: Option<String>,
    info_since: Option<Instant>,     // info_msg の表示開始時刻（3秒タイムアウト用）
    queue: VecDeque<usize>,          // 非公開、アクセサ経由で操作
    pub repeat: RepeatMode,
    playback_started_at: Option<Instant>, // is_empty() 誤検知防止の再生開始時刻
}
```

`player_state` / `queue` は直接書き換え不可。以下のメソッドで操作する:

| メソッド | 役割 |
|---------|------|
| `set_playing()` | 新規再生開始。`selected` を `playing_index` に設定。`playback_started_at` を記録 |
| `set_resumed()` | ポーズ解除。`playing_index` は変えず状態だけ `Playing` に戻す。`playback_started_at` は変更しない（ポーズ解除は新規ロードではないため誤検知ガード不要） |
| `set_paused()` / `set_stopped()` | 状態遷移。`playback_started_at` をクリア |
| `is_playback_settled()` | 再生開始から 500ms 以上経過したか。`is_empty()` の誤検知ガード。`playback_started_at` が `None` のときは `true`（= チェック許可） |
| `set_error(msg)` | `last_error` をセットし `error_since` に現在時刻を記録 |
| `set_info(msg)` | `info_msg` をセットし `info_since` に現在時刻を記録 |
| `tick_timeouts()` | `error_since` 5秒・`info_since` 3秒を過ぎていれば各メッセージを自動クリア。イベントループ先頭で毎フレーム呼ぶ |
| `clear_messages()` | `last_error` / `error_since` / `info_msg` / `info_since` を全クリア |
| `player_state()` | `PlayerState`（Copy）を値で返す。`&PlayerState` ではないため呼び出し側で `*` デリファレンス不要 |
| `enqueue_selected()` / `clear_queue()` | キュー操作 |
| `queue_len()` / `queue_is_empty()` / `queue_paths()` | キュー参照（読み取り専用） |
| `queue_positions_for(track_index)` | 指定インデックスがキューの何番目にあるかを `Vec<usize>`（1始まり）で返す。最大3件に制限 |
| `queue_badge_map()` | トラックインデックス → キュー内位置リストの `HashMap<usize, Vec<usize>>` を O(Q) で構築 |

`set_playing()` と `set_resumed()` を分けることで、ポーズ中にカーソルを別トラックへ移動してもポーズ解除時に ▶ マーカーがずれない。

### キューと RepeatMode

```
advance() の優先順位:
  1. RepeatMode::One  → playing_index のトラックをリピート。selected を playing_index に戻す
                        ※ 再生中にカーソルが移動しても元のトラックに戻る。キューは消費しない
                        ※ playing_index が None（停止中）の場合は false を返す
  2. queue に項目あり → pop_front() した index を selected にセット
  3. RepeatMode::All  → (playing_index + 1) % tracks.len()
                        ※ selected ではなく playing_index を起点にするため、キュー消費後も
                          プレイリスト全体の論理的な「次」から再開できる
  4. RepeatMode::Off  → selected + 1（末尾なら false を返して停止）
```

RepeatMode::One を最優先にすることで、1曲リピート中にキューへ追加した曲が割り込まない。  
RepeatMode::All の起点を `playing_index` にすることで、キューで途中のトラックを再生した後も  
プレイリスト順が維持される。  
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
`draw()` はフレームごとに `queue_badge_map: &HashMap<usize, Vec<usize>>` を受け取る。`event_loop` は `queue_dirty` フラグでキュー変更を検知し、`enqueue_selected` / `clear_queue` / `advance` 時のみ `queue_badge_map()` を再計算する。毎フレームの HashMap アロケートを廃止し、トラックリストループ内は `map.get(&i)` の O(1) 参照のみ行う。

### Playlist モジュール

```rust
// src/playlist.rs
pub struct Playlist {
    pub name: String,
    pub paths: Vec<PathBuf>,
}
```

- `save(&dir)` — ファイル名を ASCII 英数字・`-`・`_` のみにサニタイズして `dir/<name>.json` に保存。サニタイズ後が空文字になる場合はエラーを返す。TUI から呼ぶ際のファイル名は `playlist_<SEC>_<MS>.json`（サブ秒精度）で同一秒内の上書きを防止
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
