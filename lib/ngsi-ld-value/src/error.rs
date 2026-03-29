use thiserror::Error;

#[derive(Error, Debug)]
pub enum NgsiLdValueError {
    #[error("Invalid number format: {0}")]
    InvalidNumber(String),

    #[error("Invalid temporal format: {0}")]
    InvalidTemporal(String),

    #[error("Invalid geospatial format: {0}")]
    InvalidGeospatial(String),

    #[error("Type mismatch: expected {expected}, found {found}")]
    TypeMismatch { expected: String, found: String },

    #[error("Parsing error: {0}")]
    ParseError(String),

    #[error("Serialization error: {0}")]
    SerializationError(String),
}

pub type Result<T> = std::result::Result<T, NgsiLdValueError>;
