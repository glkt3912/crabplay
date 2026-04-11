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
  ├── pub mod config             ← Config (recent_dirs / volume / repeat / shuffle を config.toml に永続化) / xdg_config_base()
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

`seek(pos: Duration)` は `Sink::try_seek()` を呼んで指定位置にシークする。停止中（ソース未ロード）に呼ばないよう判断するのは呼び出し側（tui.rs）の責務。`SEEK_OFFSET = 5秒` は `tui.rs` モジュールレベル定数として定義。

## TUI アーキテクチャ

```
ui::tui::run()
  ├── enable_raw_mode()           キーボードの raw モード有効化
  ├── TerminalGuard（Drop guard） パニック時も必ずターミナルを復元
  │     └── drop() → disable_raw_mode() + LeaveAlternateScreen + cursor::Show
  ├── Terminal::new()             ratatui ターミナル初期化
  └── event_loop()
        ├── MarqueeCache { offset, entries }  マーキースクロール状態（offset）と col_table キャッシュ
        │     └── marquee_tick: u32            スクロール速度制御（5フレームで offset += 1）
        ├── playlist_badge_map / playlist_dirty  バッジキャッシュ（playlist 変更時のみ再計算）
        ├── Config::load() → volume / repeat / shuffle を AppState に適用 → config.save()
        ├── ui_mode / picker_entries / picker_selected  ソース選択オーバーレイの状態
        ├── name_input: String               プレイリスト名入力バッファ（NameInput モード時）
        ├── 各フレーム先頭で:
        │     ├── tick_timeouts()  info_msg 3秒 / last_error 5秒 を過ぎたら自動クリア
        │     └── playlist_dirty == true なら playlist_badge_map を再計算してフラグをリセット
        ├── terminal.draw(|f| draw(f, ..., &playlist_badge_map, &picker))
        └── event::poll(200ms)    キーイベント待機
              ├── match ui_mode
              │     ├── UiMode::Normal
              │     │     ├── clear_messages()  ← Normal キーの先頭で実行
              │     │     ├── Enter/n/p → play_current()
              │     │     │               ├── clear_messages() で全メッセージクリア
              │     │     │               └── load_and_play() 成功 → set_playing() / 失敗 → set_error() + set_stopped()
              │     │     ├── Space  → PlayerState::Stopped のときは無視
              │     │     │           Player::toggle_pause() → true → set_paused() / false → set_resumed()
              │     │     ├── ←/→   → PlayerState::Stopped のときは無視
              │     │     │           clear_messages() + Player::seek(current_pos ± SEEK_OFFSET)
              │     │     ├── ↑/↓   → AppState::next/prev()
              │     │     ├── a      → playlist_add_selected() → true: playlist_dirty + set_info("Added PL:N")
              │     │     │                                     → false: set_info("Already in playlist")
              │     │     ├── c      → clear_playlist() + playlist_dirty = true → set_info("Playlist cleared")
              │     │     ├── r      → cycle_repeat() → set_info() でモード表示（3秒）→ config.repeat 更新・save()
              │     │     ├── s      → playlist_is_empty() → true: set_error()
              │     │     │           → false: name_input.clear() + ui_mode = NameInput
              │     │     ├── z      → toggle_shuffle() → set_info() → config.shuffle 更新・save()
              │     │     ├── +/-   → volume_up/down() → player.set_volume() → config.volume 更新・save()
              │     │     ├── /      → search_query.clear() + search_indices = 全件 + ui_mode = Search
              │     │     ├── ?      → help_scroll = 0 + ui_mode = Help
              │     │     ├── o      → build_source_entries() → ui_mode = SourcePicker
              │     │     └── q      → Player::stop() → break
              │     ├── UiMode::Search
              │     │     ├── 文字   → search_query に追加 → filter_tracks() → search_cursor = 0
              │     │     ├── Backspace → search_query.pop() → filter_tracks() → search_cursor をクランプ
              │     │     ├── ↑/↓   → search_cursor を移動（フィルタ済みリスト内）
              │     │     ├── Enter  → state.selected = search_indices[search_cursor]（0件なら変更なし）→ Normal
              │     │     └── Esc    → ui_mode = Normal（selected 変更なし）
              │     ├── UiMode::SourcePicker
              │     │     ├── ↑/↓   → picker_selected を移動
              │     │     ├── Enter  → load_source() → replace_tracks() + set_info("Source loaded (playlist cleared)") → ui_mode = Normal
              │     │     ├── d      → Playlist エントリ: std::fs::remove_file() → set_info("Deleted 'name'") + picker_entries 再構築
              │     │     │           RecentDir エントリ: config.remove_recent_dir() → config.save() → set_info("Removed '...' from recents") + picker_entries 再構築
              │     │     │           Directory エントリ: set_error("Cannot delete current directory entry")
              │     │     ├── Esc    → ui_mode = Normal（変更なし）
              │     │     └── その他 → 無視
              │     ├── UiMode::Help
              │     │     ├── ↑/↓   → help_scroll をスクロール（↑: saturating_sub(1)、↓: +1）
              │     │     └── その他 → ui_mode = Normal + help_scroll = 0
              │     └── UiMode::NameInput
              │           ├── 印字可能文字（/ \ : * ? " < > | 以外）→ name_input に追加（最大200文字）
              │           ├── Backspace → name_input.pop()
              │           ├── Enter  → name_input が空: set_error() / 空でない: save_playlist() → ui_mode = Normal
              │           └── Esc    → ui_mode = Normal（保存しない）
              ├── 選択変更検知 → marquee_cache.reset_offset() / tick リセット
              ├── 5フレームごと → marquee_cache.offset += 1
              └── is_playback_settled() && rodio::Sink::empty()（再生バッファ空 = トラック完了）
                    ├── ※ is_playback_settled(): load_and_play 直後 500ms は is_empty() 誤検知を防ぐ
                    ├── clear_messages()
                    ├── advance() == true → play_current() + marquee リセット
                    └── advance() == false → set_stopped()
```

描画は `draw()` 関数で 3 ペインに分割（合計高さ: Min(3) + 4 + 3）:

- **トラックリスト** (上部 `Constraint::Min(3)`): `List` ウィジェット + `Scrollbar`。選択行ハイライト。長いタイトル・アーティスト名はマーキースクロール。各行末尾にプレイリスト位置バッジ（後述）を表示。
  - 各行の配色: 曲名 `Color::Green`、アーティスト `Color::Cyan`、時間 `Color::DarkGray`、バッジ `Color::Magenta`
  - タイトルバー: 通常時はプレイリスト件数 `" crabplay  [PL: N] "`、Search モード中はマッチ件数 `" crabplay  [検索: N/M] "` を表示
  - Search モード中はフィルタ済みトラックのみ表示。`filter_tracks(tracks, query)` が大文字小文字を無視してタイトル・アーティストを部分一致検索し、一致したインデックス列を返す
  - タイトル列・アーティスト列の幅は固定値ではなく、`chunks[0].width` からターミナル幅を取得して動的に計算（詳細は後述）
- **Now Playing** (中段 `Constraint::Length(4)`): ブロックを先に描画し `block.inner()` で内側領域を取得。内部を縦2行に分割:
  - 行1: 再生状態・曲名・アーティスト・経過時間 / 合計時間・音量。`info_msg` があれば緑色、`last_error` があれば赤色で優先表示。
  - 行2: `Gauge` ウィジェットによるプログレスバー（再生中: `Color::Yellow`、一時停止中: `Color::DarkGray`）。`info_msg` / `last_error` 表示中またはトラック未選択時は非表示。
- **キーバインド** (下段 `Constraint::Length(3)`): 現在の `repeat` モードをリアルタイム表示する動的文字列。端末幅が狭くて文字列が収まらない場合はマーキースクロール。配色 `Color::LightCyan`。
- **ソース選択オーバーレイ** (`UiMode::SourcePicker` 時のみ): `o` キーで開く中央ポップアップ。`centered_rect(70%, 60%)` で算出した領域を `Clear` でクリアしてから `draw_source_picker()` で描画。`[Dir]`（現在のソースディレクトリ）→ `[Recent]`（最近使ったディレクトリ、最大10件・`config.toml` から読み込み）→ `[PL]`（保存済みプレイリスト、mtime 降順・全件）の順に `List` で表示。ボーダー `Color::Yellow`、選択行 `bg(DarkGray) + BOLD`。`d` キーで `[PL]` エントリをディスクから削除、`[Recent]` エントリを `config.recent_dirs` から削除できる（`[Dir]` はエラー）。ディレクトリ系エントリのロード成功時に `~/.config/crabplay/config.toml` を更新する。
- **名前入力オーバーレイ** (`UiMode::NameInput` 時のみ): `s` キーで開く小型ポップアップ。`centered_rect(60%, 20%)` の領域にテキスト入力フィールドを表示。ボーダー `Color::Cyan`。Enter で保存、Esc でキャンセル。
- **ヘルプオーバーレイ** (`UiMode::Help` 時のみ): `?` キーで開く中央ポップアップ。`centered_rect(60%, 80%)` の領域を `Clear` でクリアしてから `draw_help_overlay(scroll)` で描画。通常操作・検索モード・ソースピッカー内の3セクションを `Paragraph::scroll` でスクロール可能。ボーダー `Color::Green`。↑/↓ でスクロール、他キーで閉じる。

### マーキースクロール実装

描画は `build_col_table` → `marquee_from_table` の2段階で行い、`MarqueeCache` が col_table を文字列ごとにキャッシュする。

```
build_col_table(s) -> (Vec<(累積開始列, char, 表示幅)>, total_disp)
  └── UnicodeWidthChar::width() で各文字の表示幅を計算し、累積列位置テーブルを構築

marquee_from_table(col_table, total_disp, offset, max_width) -> String
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

MarqueeCache::render(s, max_width) -> String
  ├── HashMap<String, ColTable> に s のテーブルをキャッシュ
  ├── 同一文字列の 2 フレーム目以降は build_col_table をスキップ
  └── ソース切り替え時に clear() でキャッシュ全体を解放
```

### タイトル列・アーティスト列の動的幅

```
list_inner_width = chunks[0].width - 2   // ボーダー除く
fixed_overhead   = 18                    // ボーダー2 + マーカー2 + スペース1 + 時間6 + バッジ7
available        = list_inner_width - fixed_overhead
title_width      = max(available × 62%, TITLE_MIN=20)
artist_width     = max(available - title_width, ARTIST_MIN=12)
```

ターミナルを広げると列が自動で伸び、狭くしても最低 曲名20列・アーティスト12列を確保する。

**CJK 対応パディング (`pad_display`):**  
`format!("{:<N}", s)` は char 単位でパディングするため、全角文字を含む文字列では実際の表示列数が `N` を超える。`pad_display(s, width)` は `UnicodeWidthChar::width()` で表示幅を計算し、過不足なく `width` 列に揃える。マーキーを使わない非選択行のタイトル・アーティスト列に適用。  
表示幅取得には `UnicodeWidthChar::width(ch)` を使用（`encode_utf8` バッファ不要）。

## AppState の設計

```rust
pub struct AppState {
    pub tracks: Vec<TrackInfo>,
    pub selected: usize,
    pub source_dir: PathBuf,         // 起動時スキャンディレクトリ（ソース選択で再利用）
    player_state: PlayerState,       // 非公開、遷移メソッド経由で変更
    pub last_error: Option<String>,
    error_since: Option<Instant>,    // last_error の表示開始時刻（5秒タイムアウト用）
    pub info_msg: Option<String>,
    info_since: Option<Instant>,     // info_msg の表示開始時刻（3秒タイムアウト用）
    playlist: Vec<usize>,            // 非公開、アクセサ経由で操作。再生で消費されない
    pub repeat: RepeatMode,
    playback_started_at: Option<Instant>, // is_empty() 誤検知防止の再生開始時刻
}
```

`player_state` / `playlist` は直接書き換え不可。以下のメソッドで操作する:

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
| `replace_tracks(tracks)` | ソース切り替え時にトラック一覧と全再生状態をリセット。`playlist` もクリア。`player.stop()` は呼び出し側の責務。このメソッド後に `set_info()` を呼ぶと通知メッセージを表示できる |
| `playlist_add_selected()` | 選択中トラックを `playlist` に追加。重複はスキップ。**追加されたなら `true`、既存なら `false` を返す**（TUI 側で "Already in playlist" を表示するために使用） |
| `clear_playlist()` | `playlist` を全クリア |
| `playlist_len()` / `playlist_is_empty()` / `playlist_paths()` | プレイリスト参照（読み取り専用） |
| `playlist_badge_map()` | トラックインデックス → プレイリスト内位置リストの `HashMap<usize, Vec<usize>>` を O(P) で構築 |

`set_playing()` と `set_resumed()` を分けることで、ポーズ中にカーソルを別トラックへ移動してもポーズ解除時に ▶ マーカーがずれない。

### RepeatMode と advance()

```
advance() の動作:
  1. RepeatMode::One  → playing_index のトラックをリピート。selected を playing_index に戻す
                        ※ 再生中にカーソルが移動しても元のトラックに戻る
                        ※ playing_index が None（停止中）の場合は false を返す
  2. RepeatMode::All  → (playing_index + 1) % tracks.len()
                        ※ selected ではなく playing_index を起点にするため、
                          ブラウズ中でもプレイリスト全体の論理的な「次」から再開できる
  3. RepeatMode::Off  → selected + 1（末尾なら false を返して停止）
```

`playlist`（保存用リスト）は `advance()` で消費されない。再生順制御は RepeatMode のみで行う。  
メッセージのクリアは `advance()` ではなく呼び出し側（TUI の auto-advance ブロック）の責務とする。

### プレイリスト位置バッジ

トラックリストの各行末尾に `BADGE_WIDTH = 6` 文字固定のバッジを表示する（Color::Magenta）。  
バッジはプレイリスト（`playlist: Vec<usize>`）内の位置番号を示す。

```
  Bohemian Rhapsody    Queen     5:54        ← プレイリスト未登録（空白 6 文字）
▶ Hotel California     Eagles    6:30  [1]   ← プレイリスト 1 番目
  Stairway to Heaven   Led Zep   8:02  [2]   ← プレイリスト 2 番目
```

`format_queue_badge(positions: &[usize]) -> String` の変換規則:

| 状態 | 表示例 |
|------|--------|
| 未登録 | `"      "` (空白 6 文字) |
| 1 箇所 | `"[1]   "` |
| 2 箇所かつ両方 1 桁 | `"[1,3] "` |
| それ以外 | `"[1+2] "` (先頭位置 + 残り件数) |

**パフォーマンス設計:**  
`draw()` はフレームごとに `playlist_badge_map: &HashMap<usize, Vec<usize>>` を受け取る。`event_loop` は `playlist_dirty` フラグでプレイリスト変更を検知し、`playlist_add_selected` / `clear_playlist` 時のみ `playlist_badge_map()` を再計算する。毎フレームの HashMap アロケートを廃止し、トラックリストループ内は `map.get(&i)` の O(1) 参照のみ行う。

### Playlist モジュール

```rust
// src/playlist.rs
pub struct Playlist {
    pub name: String,
    pub paths: Vec<PathBuf>,
}
```

- `save(&dir)` — 名前を `trim()` して `/ \0 制御文字` を `_` に置換したファイル名で `dir/<name>.json` に保存。trim 後が空文字の場合はエラーを返す
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
