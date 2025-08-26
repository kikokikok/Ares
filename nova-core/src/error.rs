use thiserror::Error;

/// Nova Engine error types
#[derive(Error, Debug)]
pub enum NovaError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Resource error: {0}")]
    Resource(String),

    #[error("Event system error: {0}")]
    Event(String),

    #[error("Plugin error: {0}")]
    Plugin(String),

    #[error("Threading error: {0}")]
    Threading(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] toml::de::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Generic error: {0}")]
    Generic(#[from] anyhow::Error),
}

/// Convenient Result type alias for Nova operations
pub type NovaResult<T> = Result<T, NovaError>;

impl NovaError {
    pub fn config<S: Into<String>>(msg: S) -> Self {
        NovaError::Config(msg.into())
    }

    pub fn resource<S: Into<String>>(msg: S) -> Self {
        NovaError::Resource(msg.into())
    }

    pub fn event<S: Into<String>>(msg: S) -> Self {
        NovaError::Event(msg.into())
    }

    pub fn plugin<S: Into<String>>(msg: S) -> Self {
        NovaError::Plugin(msg.into())
    }

    pub fn threading<S: Into<String>>(msg: S) -> Self {
        NovaError::Threading(msg.into())
    }
}
