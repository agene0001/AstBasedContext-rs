use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("IO error for {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("Parse error for {path}: {message}")]
    Parse { path: PathBuf, message: String },

    #[error("Unsupported language for extension: {extension}")]
    UnsupportedLanguage { extension: String },

    #[error("Tree-sitter query error: {0}")]
    Query(String),

    #[error("Graph error: {0}")]
    Graph(String),

    #[error("UTF-8 error in {path}: {source}")]
    Utf8 {
        path: PathBuf,
        source: std::str::Utf8Error,
    },
}

pub type Result<T> = std::result::Result<T, Error>;
