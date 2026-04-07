use thiserror::Error;

/// ライブラリ層で発生するエラー型。
///
/// アプリ層では `anyhow::Error` に変換して伝播させる。
/// 呼び出し元がエラー種別を `match` で判別する必要がある場合のみこの型を直接扱う。
#[derive(Debug, Error)]
pub enum AppError {
    /// `rodio` の初期化・デコードエラー。
    #[error("audio error: {0}")]
    Audio(String),

    /// `lofty` によるタグ読み取り失敗。`path` は対象ファイルパス、`message` はエラー詳細。
    #[error("metadata error: {path}: {message}")]
    Metadata { path: String, message: String },

    /// ディレクトリスキャン失敗。
    #[error("scan error: {0}")]
    Scan(String),

    /// ファイル I/O エラー。`std::io::Error` から自動変換（`#[from]`）。
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// 上記に分類されないその他のエラー。
    #[error("{0}")]
    Other(String),
}
