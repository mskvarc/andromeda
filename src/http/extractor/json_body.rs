//! NGSI-LD classification of typed JSON body extraction failures.

use crate::error::AndromedaError;
use axum::{extract::rejection::JsonRejection, http::Uri};

/// Convert an Axum JSON rejection to the clause-5.5.4 error category.
///
/// Invalid JSON syntax is an `InvalidRequest`. A valid JSON document that
/// does not deserialize to the operation's API data type is
/// `BadRequestData`, represented by the supplied domain validation factory.
pub fn invalid_json_body(rejection: JsonRejection, uri: &Uri, data_error: impl FnOnce(String) -> AndromedaError) -> AndromedaError {
    let detail = rejection.body_text();
    match rejection {
        JsonRejection::JsonDataError(_) => data_error(detail).with_instance(uri),
        JsonRejection::JsonSyntaxError(_) | JsonRejection::MissingJsonContentType(_) | JsonRejection::BytesRejection(_) => {
            AndromedaError::malformed_request(detail).with_instance(uri)
        }
        other => AndromedaError::malformed_request(other.body_text()).with_instance(uri),
    }
}
