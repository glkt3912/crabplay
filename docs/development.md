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
4. `←` / `→` で 5 秒シークし、Now Playing の経過時間が変化することを確認（停止中は無視されることも確認）
5. `n` / `p` でスキップ
6. 曲が終了したら自動で次の曲が再生されることを確認
7. 長いタイトル・アーティスト名がマーキースクロールすることを確認
8. Now Playing ペインに `[経過時間 / 合計時間]` が表示され、プログレスバーが再生位置に合わせて伸びることを確認。一時停止中はバーが灰色になることを確認
9. `a` でトラックをキューに追加し、タイトルバーに `[queue: N]` が表示されることを確認
10. `c` でキューをクリアし、タイトルバーが元に戻ることを確認
11. `r` を押すたびに下部バーのリピートモードが `Off → All → One → Off` と切り替わることを確認
12. `s` でプレイリストが `~/.config/crabplay/playlists/` に保存され、Now Playing に保存先が表示されることを確認
13. `o` で SourcePicker を開き `[Dir]` → `[Recent]` → `[PL]` の順に表示されることを確認。別ディレクトリをロード後に再度 `o` を開くと `[Recent]` に追加されることを確認
14. `+/-` で音量を変更・`r` でリピートモードを変更・`z` でシャッフルをオンにして `q` で終了。再起動後に同じ設定が復元されることを確認（`~/.config/crabplay/config.toml` の内容も確認）
15. `q` で終了し、ターミナルが正常に復元されることを確認

## CI

### チェック (`ci.yml`)

PR・main push 時に macOS × stable Rust で自動実行:

1. `cargo clippy -- -D warnings` — lint
2. `cargo build --locked` — ビルド
3. `cargo test` — テスト

### 自動フォーマット (`fmt.yml`)

PR 作成・更新時に `cargo fmt` を実行し、差分があればそのまま PR ブランチにコミットする。
手動でフォーマットを直す必要はなく、CI が自動修正する。

### リリース (`release.yml`)

`v*` タグを push すると自動的にリリースビルドを行い GitHub Releases に公開する。

## リリース手順

[Semantic Versioning](https://semver.org/)（MAJOR.MINOR.PATCH）に従う。

1. `Cargo.toml` の `version` を更新してコミット:

```bash
# 例: 0.1.0 → 0.2.0
# Cargo.toml の version = "0.2.0" に書き換え
git add Cargo.toml
git commit -m "chore: bump version to 0.2.0"
git push
```

2. タグを打って push:

```bash
git tag v0.2.0
git push origin v0.2.0
```

3. GitHub Actions の `release.yml` が自動で以下を実行:
   - `Cargo.toml` バージョンとタグの一致を検証
   - `cargo build --release` で macOS バイナリをビルド
   - `crabplay-v0.2.0-aarch64-apple-darwin.tar.gz` を生成
   - GitHub Releases を作成してバイナリを添付

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
