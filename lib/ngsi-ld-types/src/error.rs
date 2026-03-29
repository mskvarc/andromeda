use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Invalid URN: {0}")]
    InvalidUrn(#[from] urn::Error),

    #[error("Invalid attribute name: {0}")]
    InvalidAttributeName(String),

    #[error("Invalid scope: {0}")]
    InvalidScope(String),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Missing mandatory field: {0}")]
    MissingMandatoryField(String),

    #[error("Validation error: {0}")]
    Validation(String),
}

pub type Result<T> = std::result::Result<T, Error>;
