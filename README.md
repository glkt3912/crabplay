# crabplay

macOS (Apple Silicon) 向けローカル音楽プレイヤー。
ターミナルから MP3 / FLAC ファイルを再生できる TUI アプリ。

## 特徴

- MP3 / FLAC を Pure Rust デコードで再生（外部ライブラリ不要）
- リピート・シャッフル・シーク・音量調整・プレイリスト管理
- ディレクトリ / プレイリスト / 最近使ったディレクトリをその場で切り替え
- 設定（音量・リピート・シャッフル）を次回起動時に復元
- キーバインド一覧は TUI 内で `?` キーを押すと確認できる

## プロジェクト構成

```
src/
├── main.rs              エントリポイント
├── lib.rs               モジュール宣言
├── app.rs               AppState / PlayerState
├── cli.rs               clap v4 引数定義 + validate()
├── config.rs            Config 読み書き・XDG パス解決・最近使ったディレクトリ管理
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

### グローバルインストール（推奨）

```bash
git clone <repository-url>
cd crabplay
cargo install --path .
```

インストール後はどこからでも `crabplay` コマンドで起動できる（`~/.cargo/bin/` にバイナリが配置される）。

### ビルドのみ

```bash
cargo build --release
# バイナリ: target/release/crabplay
```

## 使い方

```bash
# TUI を起動（~/Music を対象）
crabplay --dir ~/Music

# トラック一覧のみ表示（TUI なし）
crabplay --dir ~/Music --list

# JSON 形式で出力
crabplay --dir ~/Music --list --format json
```

### オプション一覧

| オプション | 短縮 | 説明 | デフォルト |
|---|---|---|---|
| `--dir <DIR>` | `-d` | スキャン対象ディレクトリ | `.` (カレント) |
| `--format <FMT>` | `-f` | 出力形式 (`text` / `json`) | `text` |
| `--list` | `-l` | TUI なしでリスト出力 | `false` |
| `--help` | `-h` | ヘルプ表示 | |
| `--version` | `-V` | バージョン表示 | |


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

| クレート | バージョン | 用途 |
|---|---|---|
| `rodio` | 0.19 | 音声再生 (CoreAudio via cpal) |
| `symphonia` | — | MP3 / FLAC Pure Rust デコード (rodio 経由) |
| `lofty` | 0.21 | メタデータ (ID3/VorbisComment) 読み取り |
| `walkdir` | 2 | ディレクトリ再帰スキャン |
| `ratatui` | 0.28 | TUI フレームワーク |
| `crossterm` | 0.28 | ターミナル制御・キーイベント |
| `unicode-width` | 0.1 | CJK 全角文字対応の表示幅計算 |
| `clap` | 4 | CLI 引数定義・ヘルプ生成 |
| `anyhow` | 1 | アプリ層エラーハンドリング |
| `thiserror` | 2 | ライブラリ層エラー型定義 |
| `serde` / `serde_json` | 1 | JSON 出力 |
| `rand` | 0.8 | シャッフル再生のランダム選択 |
| `toml` | 0.8 | 設定ファイル (config.toml) の読み書き |

詳細は [docs/crate-guide.md](docs/crate-guide.md) および [docs/library-deep-dive.md](docs/library-deep-dive.md) を参照。

## 拡張ガイド

具体的な実装手順は [docs/extension-cookbook.md](docs/extension-cookbook.md) を参照。

- **対応フォーマット追加**: `Cargo.toml` に symphonia feature を追加、scanner の拡張子リストを更新
- **GUI 化**: `ui/` モジュールを `iced` 実装に差し替え
