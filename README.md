# crabplay

macOS (Apple Silicon) 向けローカル音楽プレイヤー。
ターミナルから MP3 / FLAC ファイルを再生できる TUI アプリ。

## 特徴

- **CoreAudio ネイティブ再生**: rodio + cpal 経由で macOS CoreAudio を直接使用
- **MP3 / FLAC 対応**: symphonia による Pure Rust デコード（外部 C ライブラリ不要）
- **メタデータ表示**: lofty でタイトル・アーティスト・アルバム・再生時間を取得
- **TUI**: ratatui によるキーボード操作インターフェース
- **リスト出力モード**: TUI を起動せず text / JSON 形式でトラック一覧を出力

## プロジェクト構成

```
src/
├── main.rs              エントリポイント
├── lib.rs               モジュール宣言
├── app.rs               AppState / PlayerState
├── cli.rs               clap v4 引数定義 + validate()
├── error.rs             AppError (thiserror)
├── models.rs            TrackInfo
├── output.rs            OutputFormatter トレイト (text / JSON)
├── audio/
│   └── player.rs        再生エンジン (rodio)
├── library/
│   ├── scanner.rs       ディレクトリスキャン (walkdir)
│   └── metadata.rs      メタデータ読み取り (lofty)
└── ui/
    └── tui.rs           TUI (ratatui + crossterm)
```

## インストール

```bash
git clone <repository-url>
cd crabplay
cargo build --release
```

## 使い方

```bash
# TUI を起動（~/Music を対象）
cargo run -- --dir ~/Music

# トラック一覧のみ表示（TUI なし）
cargo run -- --dir ~/Music --list

# JSON 形式で出力
cargo run -- --dir ~/Music --list --format json
```

### オプション一覧

| オプション | 短縮 | 説明 | デフォルト |
|---|---|---|---|
| `--dir <DIR>` | `-d` | スキャン対象ディレクトリ | `.` (カレント) |
| `--format <FMT>` | `-f` | 出力形式 (`text` / `json`) | `text` |
| `--list` | `-l` | TUI なしでリスト出力 | `false` |
| `--help` | `-h` | ヘルプ表示 | |
| `--version` | `-V` | バージョン表示 | |

### キーバインド (TUI)

| キー | 動作 |
|---|---|
| `↑` / `↓` | トラック選択 |
| `Enter` | 選択曲を再生 |
| `Space` | 再生 / 一時停止 |
| `n` | 次の曲 |
| `p` | 前の曲 |
| `q` | 終了 |

## 開発

```bash
# テスト
cargo test

# Clippy
cargo clippy -- -D warnings

# フォーマットチェック
cargo fmt -- --check

# リリースビルド
cargo build --release
```

## 技術スタック

| クレート | 用途 |
|---|---|
| `rodio` v0.19 | 音声再生 (CoreAudio via cpal) |
| `symphonia` | MP3 / FLAC デコード |
| `lofty` v0.21 | メタデータ読み取り |
| `walkdir` v2 | ディレクトリ再帰スキャン |
| `ratatui` v0.28 | TUI フレームワーク |
| `crossterm` v0.28 | ターミナル制御 |
| `clap` v4 | CLI 引数定義 |
| `anyhow` / `thiserror` | エラーハンドリング |
| `serde` / `serde_json` | JSON 出力 |

## 拡張ガイド

- **音量調整**: `Sink::set_volume(f32)` を Player に追加
- **シャッフル**: `rand` クレートで AppState 内の順序をシャッフル
- **進捗バー**: `Sink::get_pos()` (rodio 0.19+) で再生位置を取得
- **プレイリスト**: `AppState` に `Vec<usize>` で順序を管理
- **GUI 化**: `ui/` モジュールを `iced` 実装に差し替え
