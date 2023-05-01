use thiserror::Error;

pub type Result<T, E = CommandError> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum CommandError {
    #[error("too many unique lobbies registered")]
    TooManyLobbies,
}
