use std::fmt::Display;
use thiserror::Error;

pub type Result<T, E = LobbyCacheError> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum LobbyCacheError {
    #[error("connection: {0}")]
    Connection(#[from] tokio_tungstenite::tungstenite::error::Error),

    #[error("parsing: {0}")]
    Parsing(#[from] MessageParsingError),
}

#[derive(Debug, Error)]
pub struct MessageParsingError {
    pub message: String,
    pub error_parsing_all_messages: serde_json::Error,
    pub error_parsing_followup_message: serde_json::Error,
}

impl Display for MessageParsingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            r#"Failed to parse incoming message. error_parsing_all_messages: "{}", error_parsing_followup_message: "{}""#,
            self.error_parsing_all_messages, self.error_parsing_followup_message
        )
    }
}
