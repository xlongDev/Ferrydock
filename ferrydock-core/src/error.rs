use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("provider {0} is unhealthy (circuit open)")]
    CircuitOpen(String),

    #[error("no provider could serve the request: {0}")]
    AllFailed(String),

    #[error("upstream returned {status}: {body}")]
    Upstream { status: u16, body: String },

    #[error("request timed out")]
    Timeout,

    #[error("no model found for id '{0}'")]
    NoModel(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("config error: {0}")]
    Config(String),

    #[error("http client error: {0}")]
    Client(String),
}

pub type Result<T> = std::result::Result<T, Error>;
