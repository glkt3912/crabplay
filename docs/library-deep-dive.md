# Library Deep Dive

crabplay が依存する各ライブラリの内部設計・選定理由・重要な概念の詳細解説。

---

## rodio — 音声再生エンジン

### 概要

rodio は Rust 製の音声再生ライブラリ。内部では以下の層に分かれる:

```
アプリケーションコード
    │
    ▼
rodio::Sink          ← キューと音量・一時停止を管理
    │
    ▼
rodio::Source トレイト ← PCM データのイテレータ抽象
    │
    ▼
rodio::Decoder       ← フォーマット別デコード (symphonia に委譲)
    │
    ▼
cpal::Stream         ← OS のオーディオ API を呼び出す
    │
    ▼
CoreAudio (macOS)
```

### Sink の内部構造

`Sink` は `Arc<Controls>` を保持しており、`stop` / `pause` / `volume` などの状態をスレッドセーフに管理する。これにより `&self` で全操作が可能で、`Mutex` による外部ラップは不要。

```rust
// rodio 内部（簡略）
pub struct Controls {
    pause: AtomicBool,
    volume: Mutex<f32>,
    stopped: AtomicBool,
    position: Mutex<Duration>,
    // ...
}
```

`Sink::append()` でオーディオソースをキューに追加すると、バックグラウンドスレッドがキューを消費しながら `cpal` ストリームに PCM データを送り続ける。

### OutputStream のライフタイム問題

`OutputStream` は drop されると `cpal` ストリームが終了し、音声が止まる。これは Rust のオーナーシップ設計によるリソース管理であり、意図的な動作。

```rust
// NG: OutputStream が関数終了で drop される
fn play_bad() {
    let (_stream, handle) = OutputStream::try_default().unwrap();
    let sink = Sink::try_new(&handle).unwrap();
    sink.append(source);
    // ← _stream が drop されて音が止まる
}

// OK: Player 構造体に保持して生存期間を管理
pub struct Player {
    _stream: OutputStream,  // _ プレフィックスで「使わないが保持が必要」を明示
    sink: Sink,
}
```

### symphonia との関係

rodio の `Decoder` は複数のデコードバックエンドに対応している。`default-features = false` + `symphonia-*` feature を指定することで、Pure Rust 実装の symphonia を使う:

| feature | 対応フォーマット |
|---|---|
| `symphonia-mp3` | MP3 |
| `symphonia-flac` | FLAC |
| `symphonia-aac` | AAC |
| `symphonia-vorbis` | OGG Vorbis |
| `symphonia-wav` | WAV / AIFF |

外部 C ライブラリ（libmp3lame 等）が不要になり、クロスコンパイルや配布が容易になる。

### get_pos() の仕組み

v0.19 で追加された `Sink::get_pos()` は、デコーダから取得したサンプル数を累積して `Duration` に変換している。正確な位置情報は内部の `Controls::position` に都度書き込まれる。

---

## ratatui — TUI フレームワーク

### 概念モデル

ratatui は **即時描画モデル** (immediate mode rendering) を採用している。毎フレーム全画面を再描画し、前フレームとの差分のみを実際のターミナルに書き込む（ダブルバッファリング）。

```
terminal.draw(|frame| {
    // このクロージャが1フレームの「宣言」
    // 毎回全ウィジェットを再生成して渡す
    frame.render_widget(widget, area);
})?;
// ← draw() 内部で前フレームとの差分を計算して出力
```

React の仮想 DOM に近い考え方で、ウィジェットの状態管理はアプリ側で行い、ratatui は純粋に「どう見えるか」だけを担当する。

### Layout システム

```rust
let chunks = Layout::default()
    .direction(Direction::Vertical)
    .constraints([
        Constraint::Min(3),       // 残り全部（最低3行）
        Constraint::Length(3),    // 固定3行
        Constraint::Length(3),    // 固定3行
    ])
    .split(frame.area());
```

`Constraint` の種類:

| 種類 | 説明 |
|---|---|
| `Length(n)` | 固定n行/列 |
| `Min(n)` | 最低n行、余白は全部取る |
| `Max(n)` | 最大n行 |
| `Percentage(n)` | 全体のn% |
| `Ratio(a, b)` | a/b の割合 |

複数の `Min` が混在する場合は均等分割される。

### Stateful vs Stateless ウィジェット

- **Stateless** (`render_widget`): `Paragraph`, `Block` など。毎フレーム新規生成して渡すだけ。
- **Stateful** (`render_stateful_widget`): `List`, `Scrollbar` など。選択位置やスクロール位置を `State` 構造体に保持し、ウィジェットと分離して管理する。

```rust
// State はイベントループ側で保持
let mut list_state = ListState::default();
list_state.select(Some(0));

// 描画時に State を渡す（可変参照）
frame.render_stateful_widget(list, area, &mut list_state);
```

### Line / Span によるリッチテキスト

```rust
let line = Line::from(vec![
    Span::raw("  "),                                          // スタイルなし
    Span::styled("King Gnu", Style::default().fg(Color::White)),
    Span::styled(" — Ceremony", Style::default().fg(Color::Cyan)),
    Span::styled("  3:47", Style::default().fg(Color::DarkGray)),
]);
```

`Span` が文字スタイルの最小単位、`Line` が1行、`Text` が複数行のテキストブロック。

---

## crossterm — ターミナル制御

### Raw モードとは

通常のターミナルは「調理済みモード」（cooked mode）で動作し、Enter を押すまで入力をバッファリングする。`enable_raw_mode()` でこれを無効化し、キー入力を即座にアプリが受け取れるようにする。

```
通常モード:
  ユーザー入力 → ターミナルバッファ → Enter → アプリ

Raw モード:
  ユーザー入力 → アプリ（即座）
```

副作用として、`\n` がキャリッジリターンなしの改行になる、Ctrl+C でプロセスが即終了しなくなる等がある。必ず終了時に `disable_raw_mode()` を呼ぶ必要があり、パニック時も復元するために Drop guard を使う。

### Alternate Screen

`EnterAlternateScreen` / `LeaveAlternateScreen` は xterm 互換ターミナルが持つ「代替バッファ」への切り替えコマンド。

```
メインバッファ (通常)   │  代替バッファ (TUI 中)
                       │
$ ls                   │  ┌──────────────────┐
$ crabplay --dir ~/M   │  │  crabplay         │
                       │  │  ▶ King Gnu ...   │
                       │  └──────────────────┘
                       │
← LeaveAlternateScreen で戻る → メインバッファの内容が復元
```

TUI 終了後に以前のシェル出力が表示されるのはこの仕組みによる。

### イベントポーリング

```rust
// ノンブロッキング: 200ms 待ってイベントがなければ false を返す
if event::poll(Duration::from_millis(200))? {
    match event::read()? {
        Event::Key(key) => { /* キー処理 */ }
        Event::Resize(w, h) => { /* リサイズ処理 */ }
        _ => {}
    }
}
// poll が false の間も描画ループは継続 → アニメーションやリアルタイム更新が可能
```

200ms のタイムアウトは、CPU 使用率を抑えつつ約 5fps のリフレッシュを実現するバランス値。マーキースクロールや再生時間の更新がこの周期で行われる。

---

## unicode-width — 表示幅計算

### なぜ必要か

ターミナルはプロポーショナルフォントではなく等幅フォントを使うため、文字の「表示幅」（端末上で占める列数）が重要になる:

| 文字種 | 例 | バイト数 | 文字数 | **表示幅** |
|---|---|---|---|---|
| ASCII | `abc` | 3 | 3 | **3** |
| CJK 全角 | `日本語` | 9 | 3 | **6** |
| 絵文字 | `▶` | 3 | 1 | **1** |

`str::len()` はバイト数、`str::chars().count()` は文字数を返すが、TUI での列位置合わせには表示幅が必要。

### Unicode East Asian Width

Unicode 規格では各文字に以下の幅属性が定義されている:

- **Narrow / Halfwidth**: 幅 1（ASCII、半角カナ等）
- **Wide / Fullwidth**: 幅 2（CJK 統合漢字、全角英数等）
- **Ambiguous**: 環境依存（通常は幅 1）

`unicode-width` クレートはこの規格を実装し、`UnicodeWidthStr::width(s)` で文字列全体の表示幅を O(n) で計算する。

### マーキー実装での使い方

```rust
fn marquee_slice(s: &str, offset: usize, max_width: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut result = String::new();
    let mut width = 0usize;
    let mut idx = offset % (chars.len() + 2);  // 末尾に空白2文字分のギャップ

    loop {
        let ch = chars[idx % chars.len()];
        let mut buf = [0u8; 4];
        let ch_str: &str = ch.encode_utf8(&mut buf);
        let ch_width = UnicodeWidthStr::width(ch_str);

        if width + ch_width > max_width { break; }
        result.push(ch);
        width += ch_width;
        idx += 1;
    }
    result
}
```

---

## lofty — メタデータ読み取り

### タグフォーマットの統一

音声フォーマットごとにメタデータ規格が異なる:

| 音声フォーマット | タグ規格 |
|---|---|
| MP3 | ID3v1, ID3v2 |
| FLAC | VorbisComment |
| AAC / M4A | iTunes-style MP4 タグ |
| OGG | VorbisComment |

lofty はこれらを `Tag` トレイトで統一し、フォーマットを意識せずに読み書きできる。

### TaggedFile と primary_tag

```rust
let tagged_file = Probe::open(path)?.read()?;

// primary_tag: そのフォーマットで最も標準的なタグ
// MP3 → ID3v2 を優先（ID3v1 より情報量が多い）
// FLAC → VorbisComment
let tag = tagged_file.primary_tag();

// tags(): 全タグを取得（ID3v1 と ID3v2 の両方が埋まっている場合など）
for tag in tagged_file.tags() {
    println!("{:?}", tag.tag_type());
}
```

### AudioProperties

```rust
let props = tagged_file.properties();
let duration = props.duration();     // std::time::Duration
let bitrate  = props.audio_bitrate(); // kbps (Option<u32>)
let channels = props.channels();     // Option<u8>
```

`duration()` は音声ヘッダー（MP3 の場合は VBR ヘッダー or CBR 推算）から取得するため、完全な精度は保証されないが実用上は十分。

---

## clap — CLI フレームワーク

### derive マクロの仕組み

`#[derive(Parser)]` はコンパイル時にプロシージャルマクロが展開され、`Args::parse()` の実装が自動生成される。生成されるコードはおよそ以下に相当する:

```rust
// derive が自動生成するイメージ
impl Args {
    pub fn parse() -> Self {
        let matches = Command::new("crabplay")
            .arg(Arg::new("dir").short('d').long("dir").default_value("."))
            .arg(Arg::new("format").short('f').long("format").default_value("text"))
            .arg(Arg::new("list").short('l').long("list").action(ArgAction::SetTrue))
            .get_matches();

        Args {
            dir: matches.get_one::<PathBuf>("dir").unwrap().clone(),
            format: matches.get_one::<String>("format").unwrap().clone(),
            list: matches.get_flag("list"),
        }
    }
}
```

### バリデーションの分離

clap は型変換（文字列 → `PathBuf`）は行うが、「そのパスが存在するか」などのビジネスロジックは担当しない。そのため `Args::validate()` を別途実装し、`main.rs` で明示的に呼び出している。これにより clap の責務を「パース」に限定し、テストも容易になる。

---

## anyhow + thiserror — 二層エラー設計

### 設計の意図

```
┌─────────────────────────────────────────┐
│  main.rs (アプリ層)                      │
│  anyhow::Result → エラーチェインで表示    │
│                                         │
│  scan().context("scan failed")?         │
│  → "[error] scan failed: permission     │
│     denied (os error 13)"               │
└─────────────────────────────────────────┘
         ↑ anyhow::Error に自動変換
┌─────────────────────────────────────────┐
│  library 層                             │
│  AppError (thiserror) → 型安全なエラー   │
│                                         │
│  AppError::Scan(msg)                    │
│  AppError::Metadata { path, msg }       │
│  AppError::Audio(msg)                   │
└─────────────────────────────────────────┘
```

**thiserror** はライブラリ層で使う。呼び出し元がエラーの種類を `match` で判別できる型安全なエラー型を定義するため。

**anyhow** はアプリ層で使う。ユーザーに分かりやすいエラーメッセージを組み立てるための「エラーチェイン」機能 (`.context()`) を提供する。最終的に `eprintln!("[error] {err:#}")` でネストしたエラーを全て表示する。

### `#[from]` と `?` の組み合わせ

```rust
// error.rs
#[derive(thiserror::Error, Debug)]
pub enum AppError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),  // ← std::io::Error から自動変換
}

// metadata.rs
fn read_metadata(path: &Path) -> Result<TrackInfo, AppError> {
    let file = File::open(path)?;  // io::Error → AppError::Io に自動変換
    // ...
}
```

`#[from]` は `impl From<io::Error> for AppError` を自動実装するため、`?` でのエラー変換が透過的に行える。

---

## serde / serde_json — シリアライゼーション

### ゼロコストな derive

`#[derive(Serialize, Deserialize)]` はコンパイル時にシリアライズ / デシリアライズのコードを生成する。ランタイムリフレクションがなく、手書きと同等のパフォーマンス。

```rust
#[derive(Serialize, Deserialize)]
pub struct TrackInfo {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_secs: u64,
}

// JSON 出力
let json = serde_json::to_string_pretty(&track)?;

// 例:
// {
//   "path": "/Users/user/Music/song.mp3",
//   "title": "SPECIALZ",
//   "artist": "King Gnu",
//   "album": "CEREMONY",
//   "duration_secs": 234
// }
```

### フォーマット独立性

serde の設計思想は「データ構造」と「フォーマット」を完全に分離すること。`TrackInfo` は serde の derive さえしていれば JSON / TOML / MessagePack / CSV など任意のフォーマットに対応できる。crabplay では現在 JSON のみだが、将来 CSV や TOML 設定ファイルへの対応を追加する場合もコードの変更は最小限になる。
