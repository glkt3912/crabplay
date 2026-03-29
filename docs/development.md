# Development Guide

## セットアップ

```bash
git clone <repository-url>
cd crabplay
cargo build
```

macOS (Apple Silicon) 環境が必要。CoreAudio ドライバが標準で利用可能なため、追加のオーディオライブラリのインストールは不要。

## 動作確認

```bash
# トラックリスト表示（TUI なし）
cargo run -- --dir ~/Music --list

# JSON 出力
cargo run -- --dir ~/Music --list --format json

# TUI 起動
cargo run -- --dir ~/Music

# テスト実行
cargo test
```

## テスト

### ユニットテスト

| ファイル | テスト内容 |
|---|---|
| `src/cli.rs` | `validate()` の正常・異常ケース |
| `src/output.rs` | `TextFormatter` / `JsonFormatter` の出力検証 |
| `src/library/scanner.rs` | スキャンのパニックなし確認 |

```bash
cargo test
```

### 手動テスト

TUI の動作は自動テストが難しいため、手動で確認する:

1. `cargo run -- --dir <音楽ディレクトリ>` で TUI 起動
2. `↑` / `↓` でトラック選択
3. `Enter` で再生開始
4. `Space` で一時停止・再開
5. `n` / `p` でスキップ
6. `q` で終了

## CI

`.github/workflows/ci.yml` で以下を自動実行（macOS × stable Rust）:

1. `cargo fmt --check` — フォーマット確認
2. `cargo clippy -- -D warnings` — lint
3. `cargo build --locked` — ビルド
4. `cargo test` — テスト

## よくあるエラー

### `OutputStream::try_default()` が失敗する

macOS でオーディオデバイスが見つからない場合に発生する。
System Preferences → Sound → Output でデバイスが有効か確認する。

### MP3/FLAC ファイルが再生されない

`symphonia-mp3` / `symphonia-flac` feature が有効か `Cargo.toml` を確認する:

```toml
rodio = { version = "0.19", default-features = false, features = ["symphonia-mp3", "symphonia-flac"] }
```

### メタデータが空で表示される

タグが埋め込まれていないファイルは、ファイル名をタイトルとして表示する（`metadata.rs` のフォールバック実装）。
`lofty` の CLI ツール（`lofty-cli`）でタグを確認・編集できる。

## リリースビルド

```bash
cargo build --release
# バイナリ: target/release/crabplay
```

`Cargo.toml` の `[profile.release]` で LTO・strip・codegen-units=1 を有効化済みのため、
小サイズかつ高速なバイナリが生成される。
