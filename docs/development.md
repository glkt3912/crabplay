# Development Guide

## セットアップ

```bash
git clone <repository-url>
cd crabplay
cargo build
```

macOS (Apple Silicon) 環境が必要。CoreAudio ドライバが標準で利用可能なため、追加のオーディオライブラリのインストールは不要。

## インストール

### グローバルインストール

```bash
cargo install --path .
```

`~/.cargo/bin/crabplay` にシングルバイナリが配置される。依存ランタイム不要。
更新時は同じコマンドを再実行するだけで上書きインストールされる。

### ローカル実行（開発時）

```bash
cargo run -- --dir ~/Music
```

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
3. `Enter` で再生開始、`Space` で一時停止・再開
4. `n` / `p` でスキップ
5. 曲が終了したら自動で次の曲が再生されることを確認
6. 長いタイトル・アーティスト名がマーキースクロールすることを確認
7. Now Playing ペインに `[経過時間 / 合計時間]` が表示されることを確認
8. `q` で終了し、ターミナルが正常に復元されることを確認

## CI

`.github/workflows/ci.yml` で以下を自動実行（macOS × stable Rust）:

1. `cargo fmt --check` — フォーマット確認
2. `cargo clippy -- -D warnings` — lint
3. `cargo build --locked` — ビルド
4. `cargo test` — テスト

## よくあるエラー

### `OutputStream::try_default()` が失敗する

macOS でオーディオデバイスが見つからない場合に発生する。
System Settings → Sound → Output でデバイスが有効か確認する。

### MP3/FLAC ファイルが再生されない

`symphonia-mp3` / `symphonia-flac` feature が有効か `Cargo.toml` を確認する:

```toml
rodio = { version = "0.19", default-features = false, features = ["symphonia-mp3", "symphonia-flac"] }
```

### メタデータが空で表示される

タグが埋め込まれていないファイルは、ファイル名をタイトルとして表示する（`metadata.rs` のフォールバック実装）。
`lofty` の CLI ツール（`lofty-cli`）でタグを確認・編集できる。

### `crabplay: command not found`

`cargo install --path .` を実行後、`~/.cargo/bin` が PATH に含まれているか確認する:

```bash
echo $PATH | tr ':' '\n' | grep cargo
# 表示されない場合は ~/.zshrc または ~/.bashrc に追加:
export PATH="$HOME/.cargo/bin:$PATH"
```

### unicode-width のバージョン競合

`cargo add unicode-width` でデフォルトの最新版 (0.2.x) を追加すると ratatui の依存 (0.1.x) と競合してトレイト実装が重複する。必ず `0.1` を指定する:

```bash
cargo add unicode-width@0.1
```

## リリースビルド

```bash
cargo build --release
# バイナリ: target/release/crabplay
```

`Cargo.toml` の `[profile.release]` で LTO・strip・codegen-units=1 を有効化済みのため、
小サイズかつ高速なバイナリが生成される。
