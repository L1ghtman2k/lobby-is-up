use thiserror::Error;

pub type Result<T, E = LobbyCacheError> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum LobbyCacheError {
    #[error("connection: {0}")]
    Connection(#[from] tokio_tungstenite::tungstenite::error::Error),

    #[error("parsing: {0}")]
    Parsing(#[from] serde_json::Error),
}
