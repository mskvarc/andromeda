//! Error types for query parameter validation and parsing.

/// Errors that can occur during query parameter validation and parsing.
///
/// These map to the NGSI-LD error types defined in ETSI GS CIM 009 clause 5.6.3:
/// `BadRequestData` for semantically invalid input values, and `InvalidRequest`
/// for structurally malformed requests.
#[derive(Debug, thiserror::Error)]
pub enum QueryError {
    /// The request contains syntactically valid but semantically incorrect data.
    ///
    /// Per NGSI-LD 5.6.3.1, this covers cases such as invalid entity IDs,
    /// out-of-range values, or unknown enumeration members.
    #[error("{0}")]
    BadRequestData(String),
    /// The request structure itself is invalid.
    ///
    /// Per NGSI-LD 5.6.3.2, this covers cases such as unrecognized query options,
    /// invalid format values, or missing required parameter combinations.
    #[error("{0}")]
    InvalidRequest(String),
}
