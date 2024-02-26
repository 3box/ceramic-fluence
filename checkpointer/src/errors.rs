use actix_web::ResponseError;
use thiserror::Error as ThisError;

#[derive(ThisError, Debug)]
pub enum Error {
    #[error("Failed to Bind")]
    Bind(#[from] std::io::Error),
    #[error("Batcher Not Found: {0}")]
    NotFound(String),
    #[error("Failed to receive, shutting down")]
    Recv(#[from] tokio::sync::oneshot::error::RecvError),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Env(#[from] std::env::VarError),
    #[error("{0}")]
    Url(#[from] url::ParseError),
    #[error("{0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("{0}")]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error("{0}")]
    Ceramic(#[from] anyhow::Error),
    #[error("{0}")]
    Custom(String),
}

impl Error {
    pub fn custom<T: Into<String>>(value: T) -> Self {
        Self::Custom(value.into())
    }
}

impl ResponseError for Error {
    fn status_code(&self) -> reqwest::StatusCode {
        reqwest::StatusCode::INTERNAL_SERVER_ERROR
    }
}
