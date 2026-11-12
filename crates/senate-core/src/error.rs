use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("не найдено: {0}")]
    NotFound(String),
    #[error("{0}")]
    Invalid(String),
    #[error("недопустимый переход: {0}")]
    Transition(String),
    #[error("нет полномочий: {0}")]
    Forbidden(String),
    #[error("база данных: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

impl Error {
    pub fn invalid(msg: impl Into<String>) -> Self {
        Error::Invalid(msg.into())
    }
    pub fn not_found(what: impl Into<String>) -> Self {
        Error::NotFound(what.into())
    }
}
