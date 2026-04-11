# crabplay

macOS (Apple Silicon) 向けローカル音楽プレイヤー。
ターミナルから MP3 / FLAC ファイルを再生できる TUI アプリ。

## 特徴

- **CoreAudio ネイティブ再生**: rodio + cpal 経由で macOS CoreAudio を直接使用
- **MP3 / FLAC 対応**: symphonia による Pure Rust デコード（外部 C ライブラリ不要）
- **メタデータ表示**: lofty でタイトル・アーティスト・アルバム・再生時間を取得
- **TUI**: ratatui によるキーボード操作インターフェース
- **再生時間表示**: Now Playing ペインに経過時間 / 合計時間をリアルタイム表示
- **プログレスバー**: Now Playing ペインに再生位置をバーで視覚表示
- **シーク**: `←` / `→` キーで ±5 秒シーク
- **インクリメンタル検索**: `/` キーでタイトル・アーティストをリアルタイム検索・絞り込み
- **ヘルプオーバーレイ**: `?` キーで全キーバインド一覧をポップアップ表示（スクロール対応）
- **マーキースクロール**: 列幅を超えるタイトル・アーティスト名を選択中に自動スクロール（CJK 対応）
- **自動次曲再生**: 曲が終わると自動的に次のトラックを再生
- **リピートモード**: Off / All / One の 3 段階をサイクル切り替え
- **シャッフル再生**: ランダムトラック選択をトグルで切り替え
- **音量調整**: `+` / `-` キーで ±5%（0〜200%）、Now Playing に現在音量を表示
- **設定永続化**: 音量・リピートモード・シャッフル状態を `~/.config/crabplay/config.toml` に保存し次回起動時に復元
- **プレイリスト管理**: トラックの追加・保存・読み込み・削除（`~/.config/crabplay/playlists/` に JSON 保存）
- **キュービューアー**: `v` キーでキュー内容をオーバーレイ表示。個別削除も可能
- **欠損ファイル通知**: プレイリスト読み込み時に存在しないファイルがあればスキップ数を通知
- **ソース切り替え**: ディレクトリ・最近使ったディレクトリ（最大10件）・プレイリストをオーバーレイ UI から動的に切り替え
- **リスト出力モード**: TUI を起動せず text / JSON 形式でトラック一覧を出力

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

### キーバインド (TUI)

#### 通常操作

| キー | 動作 |
|---|---|
| `↑` / `↓` | トラック選択 |
| `Enter` | 選択曲を再生 |
| `Space` | 再生 / 一時停止 |
| `←` / `→` | ±5 秒シーク |
| `n` | 次の曲へスキップして再生 |
| `p` | 前の曲へスキップして再生 |
| `r` | リピートモード切り替え (Off → All → One) |
| `z` | シャッフル On / Off |
| `+` / `-` | 音量 +5% / -5% |
| `/` | インクリメンタル検索モードに入る |
| `a` | 選択曲をプレイリストに追加 |
| `c` | プレイリストをクリア |
| `v` | キュービューアーを開く |
| `s` | プレイリストを名前をつけて保存 |
| `o` | ソースピッカーを開く（ディレクトリ / プレイリスト切り替え） |
| `?` | キーバインドヘルプを表示 |
| `q` | 終了 |

#### ヘルプオーバーレイ内 (`?` キーで開く)

| キー | 動作 |
|---|---|
| `↑` / `↓` | スクロール |
| 任意のキー | ヘルプを閉じる |

#### 検索モード (`/` キーで入る)

| キー | 動作 |
|---|---|
| 文字入力 | クエリに追加してリアルタイム絞り込み |
| `Backspace` | クエリを 1 文字削除して再絞り込み |
| `↑` / `↓` | 絞り込み結果内を移動 |
| `Enter` | 選択を確定して通常モードに戻る |
| `Esc` | 検索をキャンセルして元の選択に戻る |

#### キュービューアー内 (`v` キーで開く)

| キー | 動作 |
|---|---|
| `↑` / `↓` | 項目選択 |
| `d` | 選択中のトラックをキューから削除 |
| `Esc` | ビューアーを閉じる |

#### ソースピッカー内

| キー | 動作 |
|---|---|
| `↑` / `↓` | 項目選択 |
| `Enter` | 選択したソースを読み込む |
| `d` | 選択中のプレイリストを削除 / `[Recent]` を履歴から削除 |
| `Esc` | ピッカーを閉じる |

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
