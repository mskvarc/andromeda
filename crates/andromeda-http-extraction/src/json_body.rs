//! Policy-independent classification of typed JSON body extraction failures.

use axum::extract::rejection::JsonRejection;

/// Classification of a typed JSON body extraction failure.
#[derive(Debug)]
pub enum JsonBodyFailure {
    /// The request is not a syntactically valid JSON request.
    InvalidRequest(String),
    /// The JSON document is valid but does not match the requested wire type.
    BadRequestData(String),
}

impl JsonBodyFailure {
    /// Project the transport classification through caller-owned error constructors.
    pub fn map<T>(self, invalid_request: impl FnOnce(String) -> T, bad_request_data: impl FnOnce(String) -> T) -> T {
        match self {
            JsonBodyFailure::InvalidRequest(detail) => invalid_request(detail),
            JsonBodyFailure::BadRequestData(detail) => bad_request_data(detail),
        }
    }
}

/// Classify an Axum JSON rejection according to NGSI-LD clause 5.5.2.
#[must_use]
pub fn classify_json_body(rejection: JsonRejection) -> JsonBodyFailure {
    let detail = rejection.body_text();
    if let JsonRejection::JsonDataError(_) = rejection {
        JsonBodyFailure::BadRequestData(detail)
    } else {
        JsonBodyFailure::InvalidRequest(detail)
    }
}
