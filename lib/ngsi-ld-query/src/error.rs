#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    #[error("{0}")]
    BadRequestData(String),
    #[error("{0}")]
    InvalidRequest(String),
}
